//! HTTPS client for the registry lookups (SPEC-n33 section 3.7): `UreqFetcher`, the allow-list check,
//! capped reads and the app-wide rate window.
//!
//! This is the only file in `package_check/` that opens a socket. Every GET passes `check_url` first, so a URL
//! that is not a registry base (or whose path climbs out of it) never reaches the network. Redirects are not
//! followed, no proxy is used, and no cookies are kept. The request carries exactly `User-Agent`, `Accept`
//! and the `Host` ureq adds; nothing about the project, the agent or the pane.

use std::io::{self, Read};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::model::{base_for, FetchError, Fetcher, Got, CONNECT_MS, GET_MS, PUBLIC_BASE, RATE_PER_MIN, USER_AGENT_PREFIX};

/// The window `RATE_PER_MIN` GETs fall in.
const RATE_WINDOW_MS: u64 = 60_000;

/// Fixed window over the whole app. A poisoned lock refuses (fails closed).
pub struct RateWindow {
    /// (start of the current window in ms, GETs taken in it).
    state: Mutex<(Option<u64>, u32)>,
}

impl RateWindow {
    pub const fn new() -> Self {
        Self { state: Mutex::new((None, 0)) }
    }

    /// True when a GET may go now. The `RATE_PER_MIN`-th and earlier calls in a window pass; the next one
    /// is refused until the window has run `RATE_WINDOW_MS`.
    pub fn take(&self, now_ms: u64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let (start, count) = &mut *state;
        let fresh = match *start {
            Some(started) => now_ms.saturating_sub(started) >= RATE_WINDOW_MS,
            None => true,
        };
        if fresh {
            *start = Some(now_ms);
            *count = 0;
        }
        if *count < RATE_PER_MIN {
            *count += 1;
            true
        } else {
            false
        }
    }
}

/// The app-wide window every `UreqFetcher::new()` shares.
static RATE: RateWindow = RateWindow::new();

/// The real network seam for the lookups.
pub struct UreqFetcher {
    agent: ureq::Agent,
    rate: &'static RateWindow,
    /// Loopback bases a test fetcher may call. Only in test builds.
    #[cfg(test)]
    local: Vec<String>,
}

impl UreqFetcher {
    /// The fetcher the app uses: the app-wide rate window, public registries only.
    pub fn new() -> Self {
        Self {
            agent: agent(),
            rate: &RATE,
            #[cfg(test)]
            local: Vec::new(),
        }
    }
}

impl Fetcher for UreqFetcher {
    fn get(&self, url: &str, accept: &str, max_bytes: usize) -> Result<Got, FetchError> {
        #[cfg(test)]
        let local: &[String] = &self.local;
        #[cfg(not(test))]
        let local: &[String] = &[];

        // Refused before any socket, and before the rate window is spent.
        check_url(url, local)?;
        if !self.rate.take(now_ms()) {
            return Err(FetchError::TooMany);
        }
        let sent = self
            .agent
            .get(url)
            .set("User-Agent", &user_agent())
            .set("Accept", accept)
            .call();
        match sent {
            Ok(response) if response.status() == 200 => {
                read_capped(response.into_reader(), max_bytes).map(Got::Ok)
            }
            // Any other 2xx, and every 3xx (redirects are not followed, so the Location is never read).
            Ok(_) => Err(FetchError::Unreadable),
            Err(ureq::Error::Status(code, _response)) => status_error(code),
            // The transport text is never formatted: it names the URL, which names the package.
            Err(ureq::Error::Transport(transport)) => Err(if is_timeout(&transport) {
                FetchError::Timeout
            } else {
                FetchError::Unreachable
            }),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(CONNECT_MS))
        .timeout(Duration::from_millis(GET_MS))
        // A redirect would send the request to an address the allow-list never checked.
        .redirects(0)
        // No proxy: an environment proxy would carry the package name to a host nobody chose.
        .try_proxy_from_env(false)
        .build()
}

/// The allow-list (SPEC-n33 S5). Decides from the URL text alone; touches no socket.
///
/// A URL passes only when it starts with a base followed by `/` and a path with no `.` or `..` segments.
/// Bases are the public constants, what `base_for` gives now, and `local` (test fetchers only). A base must be
/// `https://`; the only other scheme allowed is `http://127.0.0.1:` in a test or dev build.
pub fn check_url(url: &str, local: &[String]) -> Result<(), FetchError> {
    // Control characters, spaces and backslashes never belong in a registry URL.
    if url.bytes().any(|b| b <= b' ' || b == 0x7f || b == b'\\') {
        return Err(FetchError::NotAllowed);
    }
    let allowed = allowed_bases(local).iter().any(|base| {
        url.strip_prefix(base.as_str())
            .is_some_and(|path| scheme_ok(base) && path.starts_with('/') && !has_dot_segment(path))
    });
    if allowed {
        Ok(())
    } else {
        Err(FetchError::NotAllowed)
    }
}

/// Every base a GET may start with. A test base can replace a public one through `base_for`.
fn allowed_bases(local: &[String]) -> Vec<String> {
    let mut bases: Vec<String> = PUBLIC_BASE.iter().map(|(_, base)| (*base).to_string()).collect();
    bases.extend(PUBLIC_BASE.iter().map(|(registry, _)| base_for(*registry)));
    bases.extend(local.iter().cloned());
    bases
}

fn scheme_ok(base: &str) -> bool {
    base.starts_with("https://")
        || (cfg!(any(test, feature = "dev-fake-tui")) && base.starts_with("http://127.0.0.1:"))
}

fn has_dot_segment(path: &str) -> bool {
    path.split('/').any(|segment| {
        matches!(
            segment.to_ascii_lowercase().as_str(),
            "." | ".." | "%2e" | "%2e%2e" | ".%2e" | "%2e."
        )
    })
}

/// Reads at most `max_bytes`. One byte more is read to tell "exactly at the cap" from "over it".
pub fn read_capped(reader: impl Read, max_bytes: usize) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    let read = reader
        .take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes);
    if bytes.len() > max_bytes {
        return Err(FetchError::TooBig);
    }
    match read {
        Ok(_) => Ok(bytes),
        Err(err) if is_timeout_kind(err.kind()) => Err(FetchError::Timeout),
        Err(_) => Err(FetchError::Unreadable),
    }
}

/// What a status code means for a lookup. Bodies of error answers are never read.
fn status_error(code: u16) -> Result<Got, FetchError> {
    match code {
        404 | 410 => Ok(Got::NotFound),
        429 => Err(FetchError::TooMany),
        401 | 403 => Err(FetchError::Refused),
        // 3xx, 5xx and anything else.
        _ => Err(FetchError::Unreadable),
    }
}

/// ureq wraps a timeout in its source chain (a connect timeout, or a read that ran out of time).
fn is_timeout(transport: &ureq::Transport) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(transport);
    while let Some(err) = source {
        if let Some(io_err) = err.downcast_ref::<io::Error>() {
            return is_timeout_kind(io_err.kind());
        }
        source = err.source();
    }
    false
}

fn is_timeout_kind(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
}

fn user_agent() -> String {
    format!("{USER_AGENT_PREFIX}{}", env!("CARGO_PKG_VERSION"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;

    const ACCEPT: &str = "application/vnd.pypi.simple.v1+json";

    /// What the local test server answers for one request.
    #[derive(Clone)]
    struct Canned {
        status_line: String,
        /// Extra header lines, each ending in CRLF.
        extra_headers: String,
        body: Vec<u8>,
        delay: Duration,
    }

    fn canned(status_line: &str, body: &[u8]) -> Canned {
        Canned {
            status_line: status_line.to_string(),
            extra_headers: String::new(),
            body: body.to_vec(),
            delay: Duration::ZERO,
        }
    }

    /// One request the local server read: the request line and the headers (lowercase names).
    #[derive(Debug, Clone)]
    struct Seen {
        request_line: String,
        headers: Vec<(String, String)>,
    }

    struct LocalServer {
        /// `http://127.0.0.1:<port>`
        base: String,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    /// A server on 127.0.0.1 with a free port. One thread per connection, so a slow answer does not hold up others.
    fn serve(route: impl Fn(&str) -> Canned + Send + Sync + 'static) -> LocalServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let route = Arc::new(route);
        let seen_for_loop = seen.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let route = route.clone();
                let seen = seen_for_loop.clone();
                thread::spawn(move || handle(stream, &*route, &seen));
            }
        });
        LocalServer { base: format!("http://127.0.0.1:{port}"), seen }
    }

    fn handle(mut stream: TcpStream, route: &dyn Fn(&str) -> Canned, seen: &Mutex<Vec<Seen>>) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        // Read the request head byte by byte up to the blank line.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 16 * 1024 {
                return;
            }
            match stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => return,
            }
        }
        let text = String::from_utf8_lossy(&head).into_owned();
        let mut lines = text.split("\r\n");
        let request_line = lines.next().unwrap_or("").to_string();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
            .collect();
        let path = request_line.split(' ').nth(1).unwrap_or("").to_string();
        if let Ok(mut log) = seen.lock() {
            log.push(Seen { request_line, headers });
        }

        let answer = route(&path);
        if !answer.delay.is_zero() {
            thread::sleep(answer.delay);
        }
        let head = format!(
            "{}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
            answer.status_line,
            answer.extra_headers,
            answer.body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&answer.body);
        let _ = stream.flush();
    }

    /// A fetcher whose only extra base is the local server, with its own rate window.
    fn fetcher_for(base: &str) -> UreqFetcher {
        let rate: &'static RateWindow = Box::leak(Box::new(RateWindow::new()));
        UreqFetcher { agent: agent(), rate, local: vec![base.to_string()] }
    }

    fn fetch(base: &str, path: &str, max_bytes: usize) -> Result<Got, FetchError> {
        fetcher_for(base).get(&format!("{base}{path}"), ACCEPT, max_bytes)
    }

    fn header(seen: &Seen, name: &str) -> String {
        seen.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    #[test]
    fn two_hundred_returns_the_body() {
        let server = serve(|_| canned("HTTP/1.1 200 OK", b"{\"name\":\"made-up-pkg\"}"));
        assert_eq!(
            fetch(&server.base, "/made-up-pkg", 1024),
            Ok(Got::Ok(b"{\"name\":\"made-up-pkg\"}".to_vec()))
        );
    }

    #[test]
    fn status_codes_map_to_the_fetch_answers() {
        let cases: [(&str, Result<Got, FetchError>); 9] = [
            ("HTTP/1.1 404 Not Found", Ok(Got::NotFound)),
            ("HTTP/1.1 410 Gone", Ok(Got::NotFound)),
            ("HTTP/1.1 429 Too Many Requests", Err(FetchError::TooMany)),
            ("HTTP/1.1 403 Forbidden", Err(FetchError::Refused)),
            ("HTTP/1.1 401 Unauthorized", Err(FetchError::Refused)),
            ("HTTP/1.1 500 Internal Server Error", Err(FetchError::Unreadable)),
            ("HTTP/1.1 503 Service Unavailable", Err(FetchError::Unreadable)),
            ("HTTP/1.1 204 No Content", Err(FetchError::Unreadable)),
            ("HTTP/1.1 418 I'm a teapot", Err(FetchError::Unreadable)),
        ];
        for (status_line, expected) in cases {
            let server = serve(move |_| canned(status_line, b"{}"));
            assert_eq!(fetch(&server.base, "/made-up-pkg", 1024), expected, "{status_line}");
        }
    }

    #[test]
    fn redirect_is_not_followed() {
        let server = serve(|path| {
            if path == "/moved" {
                Canned {
                    extra_headers: "Location: /elsewhere\r\n".to_string(),
                    ..canned("HTTP/1.1 302 Found", b"")
                }
            } else {
                canned("HTTP/1.1 200 OK", b"{}")
            }
        });
        assert_eq!(fetch(&server.base, "/moved", 1024), Err(FetchError::Unreadable));
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "the redirect target must not be requested");
        assert_eq!(seen[0].request_line, "GET /moved HTTP/1.1");
    }

    #[test]
    fn answer_slower_than_get_ms_times_out() {
        let server = serve(|_| Canned {
            delay: Duration::from_millis(GET_MS + 2_000),
            ..canned("HTTP/1.1 200 OK", b"{}")
        });
        assert_eq!(fetch(&server.base, "/slow", 1024), Err(FetchError::Timeout));
    }

    #[test]
    fn body_over_the_cap_is_too_big_and_exactly_at_the_cap_is_kept() {
        let body = vec![b'a'; 2048];
        let server = serve(move |_| canned("HTTP/1.1 200 OK", &body));
        assert_eq!(fetch(&server.base, "/big", 1024), Err(FetchError::TooBig));
        assert_eq!(fetch(&server.base, "/big", 2048), Ok(Got::Ok(vec![b'a'; 2048])));
    }

    #[test]
    fn request_carries_only_user_agent_and_accept() {
        let server = serve(|_| canned("HTTP/1.1 200 OK", b"{}"));
        assert!(fetch(&server.base, "/made-up-pkg", 1024).is_ok());
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let request = &seen[0];
        assert_eq!(request.request_line, "GET /made-up-pkg HTTP/1.1");
        // ureq 2 adds `Accept-Encoding: gzip` itself (its default `gzip` feature, kept on in Cargo.toml).
        // Nothing else: no cookie, authorization, referer or project data.
        let names: BTreeSet<&str> = request.headers.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, BTreeSet::from(["accept", "accept-encoding", "host", "user-agent"]));
        assert_eq!(header(request, "accept-encoding"), "gzip");
        assert_eq!(
            header(request, "user-agent"),
            format!("PuppetMaster-PackageCheck/{}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(header(request, "accept"), ACCEPT);
        assert_eq!(header(request, "host"), server.base.trim_start_matches("http://"));
    }

    #[test]
    fn urls_off_the_list_are_refused_before_any_socket() {
        // A loopback listener that is NOT a base of the fetcher: a refused URL must never connect to it.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("local addr").port();
        let fetcher = UreqFetcher::new();
        let urls = [
            "https://evil.example/x".to_string(),
            "http://pypi.org/simple/made-up-pkg/".to_string(),
            "https://pypi.org.evil.example/simple/made-up-pkg/".to_string(),
            "https://registry.npmjs.org@evil.example/made-up-pkg".to_string(),
            "https://pypi.org:443/simple/made-up-pkg/".to_string(),
            "https://registry.npmjs.org/../../evil.example/x".to_string(),
            "https://pypi.org/simple/%2e%2e/x/".to_string(),
            "https://crates.io\\evil.example/x".to_string(),
            format!("http://127.0.0.1:{port}/made-up-pkg"),
        ];
        for url in &urls {
            assert_eq!(fetcher.get(url, ACCEPT, 1024), Err(FetchError::NotAllowed), "{url}");
        }
        assert!(listener.accept().is_err(), "a refused URL must not open a connection");
    }

    #[test]
    fn public_registry_urls_pass_the_check_without_a_request() {
        for (registry, base) in PUBLIC_BASE {
            let url = format!("{base}/made-up-pkg");
            assert_eq!(check_url(&url, &[]), Ok(()), "{registry:?}");
        }
        assert_eq!(check_url("https://pypi.org/simple/made-up-pkg/", &[]), Ok(()));
        assert_eq!(check_url("http://127.0.0.1:9/x", &[]), Err(FetchError::NotAllowed));
    }

    #[test]
    fn rate_window_refuses_the_61st_call_and_reopens_after_a_minute() {
        let rate = RateWindow::new();
        for _ in 0..RATE_PER_MIN {
            assert!(rate.take(1_000));
        }
        assert!(!rate.take(1_000), "the 61st call in one window is refused");
        assert!(rate.take(1_000 + RATE_WINDOW_MS), "a new window starts after a minute");
    }

    #[test]
    fn a_fetch_over_the_rate_makes_no_request() {
        let server = serve(|_| canned("HTTP/1.1 200 OK", b"{}"));
        let rate: &'static RateWindow = Box::leak(Box::new(RateWindow::new()));
        for _ in 0..RATE_PER_MIN {
            assert!(rate.take(now_ms()));
        }
        let fetcher = UreqFetcher { agent: agent(), rate, local: vec![server.base.clone()] };
        let url = format!("{}/made-up-pkg", server.base);
        assert_eq!(fetcher.get(&url, ACCEPT, 1024), Err(FetchError::TooMany));
        assert!(server.seen.lock().unwrap().is_empty(), "no socket for a refused call");
    }

    #[test]
    fn read_capped_keeps_exactly_the_cap_and_refuses_one_more_byte() {
        assert_eq!(read_capped(io::Cursor::new(vec![7u8; 10]), 10), Ok(vec![7u8; 10]));
        assert_eq!(read_capped(io::Cursor::new(vec![7u8; 11]), 10), Err(FetchError::TooBig));
    }

    /// A reader that fails the way a timed out or broken body does.
    struct Failing(io::ErrorKind);

    impl Read for Failing {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "read failed"))
        }
    }

    #[test]
    fn read_errors_map_to_timeout_or_unreadable() {
        assert_eq!(read_capped(Failing(io::ErrorKind::TimedOut), 10), Err(FetchError::Timeout));
        assert_eq!(read_capped(Failing(io::ErrorKind::ConnectionReset), 10), Err(FetchError::Unreadable));
    }
}
