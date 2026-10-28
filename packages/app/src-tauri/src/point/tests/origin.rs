//! Point and Fix tests for origin: the local-only URL rule (S1), the navigation guard (S2) and
//! Puppet Master's own ports (S4).
use crate::pane_preview::PreviewUrl;
use crate::point::origin::{allowed, cdp_port_in, nav, resolve_open, Nav, OwnPorts, Refusal};

fn own() -> OwnPorts {
    OwnPorts::current(Some(5173))
}

fn preview(url: &str, port: u16) -> PreviewUrl {
    PreviewUrl { url: url.to_string(), port, network_url: None }
}

#[test]
fn allows_loopback_hosts_over_http_and_https() {
    for url in [
        "http://localhost:4000/",
        "https://localhost:4000/pricing?x=1#top",
        "http://127.0.0.1:4000",
        "http://[::1]:4000/",
        "HTTP://LOCALHOST:4000/",
    ] {
        assert!(allowed(url, &own()).is_ok(), "should allow {url}");
    }
}

#[test]
fn writes_the_default_port_for_the_scheme() {
    assert_eq!(allowed("http://LocalHost/", &own()).unwrap().as_str(), "http://localhost:80");
    assert_eq!(allowed("https://127.0.0.1", &own()).unwrap().as_str(), "https://127.0.0.1:443");
    assert_eq!(allowed("http://[::1]:4000/x", &own()).unwrap().as_str(), "http://[::1]:4000");
}

#[test]
fn refuses_lan_public_and_other_non_loopback_hosts() {
    for url in [
        "http://192.168.1.20:5173/",
        "http://10.0.0.5:3000",
        "http://172.16.4.2:3000",
        "https://example.com/",
        "http://0.0.0.0:3000",
        "http://127.0.0.2:3000",
        "http://[fe80::1]:3000",
        "http://evil.example/@localhost:4000",
    ] {
        assert_eq!(allowed(url, &own()), Err(Refusal::NotLocal), "should refuse {url}");
    }
}

#[test]
fn refuses_lookalike_hosts_as_typed() {
    for url in [
        "http://localhost.evil.com:4000",
        "http://localhost.:4000",
        "http://0x7f.1:4000",
        "http://127.1:4000",
        "http://2130706433:4000",
        "http://localhost%2eevil.com:4000",
        "http://localhost\\.evil.com:4000",
        "http://\u{ff4c}ocalhost:4000",
        "http://xn--localhost-5r0b:4000",
    ] {
        assert_eq!(allowed(url, &own()), Err(Refusal::NotLocal), "should refuse {url}");
    }
}

#[test]
fn refuses_user_info_tricks() {
    for url in [
        "http://localhost@evil.example:4000/",
        "http://evil@localhost:4000",
        "http://localhost\\@evil.com:4000",
        "http://user:pw@localhost:4000",
    ] {
        assert_eq!(allowed(url, &own()), Err(Refusal::BadUrl), "should refuse {url}");
    }
}

#[test]
fn refuses_non_http_schemes_and_plain_text() {
    for url in [
        "file:///C:/Windows/win.ini",
        "javascript:alert(1)",
        "data:text/html,<h1>x</h1>",
        "ftp://localhost:21/",
        "ws://localhost:4000",
        "about:blank",
        "tauri://localhost",
        "//localhost:4000",
        "localhost:4000",
        "",
    ] {
        assert_eq!(allowed(url, &own()), Err(Refusal::BadUrl), "should refuse {url}");
    }
}

#[test]
fn refuses_bad_ports_and_whitespace() {
    for url in [
        "http://localhost:0/",
        "http://localhost:65536/",
        "http://localhost:abc/",
        "http://localhost:/",
        "http://localhost:+4000/",
        "http://localhost:-1/",
        "http://localhost:4000:80/",
        "http://localhost:400 0/",
        " http://localhost:4000",
        "http://localhost:4000\n",
        "http://[::1]4000/",
        "http://localhost:000000001/",
    ] {
        assert_eq!(allowed(url, &own()), Err(Refusal::BadUrl), "should refuse {url:?}");
    }
}

#[test]
fn refuses_own_ports() {
    for url in [
        "http://localhost:1420/",
        "https://localhost:1420/",
        "http://127.0.0.1:17321/",
        "http://localhost:17322",
        "http://localhost:17350/",
        "http://localhost:17399/",
        "http://localhost:9333/",
        "http://localhost:5173/",
    ] {
        assert_eq!(allowed(url, &own()), Err(Refusal::OwnPort), "should refuse {url}");
    }
}

#[test]
fn allows_ports_just_outside_our_ranges() {
    for port in [17320, 17400, 4000, 3000, 80, 65535] {
        let url = format!("http://localhost:{port}/");
        assert!(allowed(&url, &own()).is_ok(), "should allow {url}");
    }
}

#[test]
fn own_ports_include_the_debugging_port_from_args() {
    assert_eq!(cdp_port_in("--remote-debugging-port=9222"), Some(9222));
    assert_eq!(cdp_port_in("--foo --remote-debugging-port=9444 --bar"), Some(9444));
    assert_eq!(cdp_port_in("--remote-debugging-port=0"), None);
    assert_eq!(cdp_port_in("--remote-debugging-port=99999"), None);
    assert_eq!(cdp_port_in("--remote-debugging-port="), None);
    assert_eq!(cdp_port_in("--remote-debugging-port-x=9222"), None);
    assert_eq!(cdp_port_in(""), None);
    let ports = OwnPorts::current(None);
    assert!(ports.contains(1420));
    assert!(ports.contains(9333));
    assert!(ports.contains(17321));
    assert!(!ports.contains(4000));
}

#[test]
fn matches_only_the_same_origin() {
    let origin = allowed("http://localhost:4000/app", &own()).unwrap();
    assert!(origin.matches("http://localhost:4000/pricing?x=1"));
    assert!(!origin.matches("https://localhost:4000/"));
    assert!(!origin.matches("http://127.0.0.1:4000/"));
    assert!(!origin.matches("http://localhost:4001/"));
    assert!(!origin.matches("http://localhost.evil.com:4000/"));
    assert!(!origin.matches("http://localhost@evil.com:4000/"));
    assert!(!origin.matches("about:blank"));
}

#[test]
fn resolve_open_uses_the_printed_local_url() {
    let previews = [preview("http://localhost:4000/", 4000)];
    assert_eq!(resolve_open(4000, &previews, &[]), Ok("http://localhost:4000/".to_string()));
}

#[test]
fn resolve_open_uses_a_job_port_as_localhost() {
    assert_eq!(resolve_open(3000, &[], &[3000]), Ok("http://localhost:3000/".to_string()));
}

#[test]
fn resolve_open_refuses_a_port_nobody_is_serving() {
    assert_eq!(resolve_open(4000, &[], &[]), Err(Refusal::NotRunning));
}

#[test]
fn resolve_open_refuses_a_lan_only_address() {
    let previews = [PreviewUrl {
        url: "http://192.168.1.20:4000/".to_string(),
        port: 4000,
        network_url: Some("http://192.168.1.20:4000/".to_string()),
    }];
    assert_eq!(resolve_open(4000, &previews, &[]), Err(Refusal::NotLocal));
}

#[test]
fn resolve_open_refuses_own_ports_from_either_list() {
    assert_eq!(resolve_open(1420, &[preview("http://localhost:1420/", 1420)], &[]), Err(Refusal::OwnPort));
    assert_eq!(resolve_open(17321, &[], &[17321]), Err(Refusal::OwnPort));
}

#[test]
fn resolve_open_refuses_a_url_on_another_port() {
    let previews = [preview("http://localhost:4001/", 4000)];
    assert_eq!(resolve_open(4000, &previews, &[]), Err(Refusal::BadUrl));
}

#[test]
fn every_refusal_has_a_plain_sentence_without_a_url() {
    for refusal in [Refusal::NotRunning, Refusal::NotLocal, Refusal::OwnPort, Refusal::BadUrl] {
        let message = refusal.message();
        assert!(message.ends_with('.'), "{message}");
        assert!(!message.contains("http"), "{message}");
    }
}

#[test]
fn nav_allows_the_same_origin_about_blank_and_other_local_apps() {
    let origin = allowed("http://localhost:4000/", &own()).unwrap();
    assert_eq!(nav(&origin, "http://localhost:4000/pricing"), Nav::Allow);
    assert_eq!(nav(&origin, "about:blank"), Nav::Allow);
    assert_eq!(nav(&origin, "http://127.0.0.1:4001/"), Nav::Allow);
}

#[test]
fn nav_blocks_other_sites_and_schemes() {
    let origin = allowed("http://localhost:4000/", &own()).unwrap();
    assert_eq!(nav(&origin, "https://example.com/login"), Nav::Block { host: "example.com".into() });
    assert_eq!(nav(&origin, "file:///C:/x.html"), Nav::Block { host: "an unknown address".into() });
    assert_eq!(nav(&origin, "javascript:alert(1)"), Nav::Block { host: "an unknown address".into() });
    assert_eq!(nav(&origin, "http://192.168.1.5:3000"), Nav::Block { host: "192.168.1.5".into() });
}

#[test]
fn nav_blocks_our_own_ports_even_on_localhost() {
    let origin = allowed("http://localhost:4000/", &own()).unwrap();
    assert_eq!(nav(&origin, "http://localhost:17321/"), Nav::Block { host: "localhost".into() });
    assert_eq!(nav(&origin, "http://[::1]:9333/"), Nav::Block { host: "::1".into() });
}

#[test]
fn nav_block_notice_never_shows_user_info_path_or_query() {
    let origin = allowed("http://localhost:4000/", &own()).unwrap();
    assert_eq!(
        nav(&origin, "https://user:secret@example.com/?token=abc"),
        Nav::Block { host: "example.com".into() }
    );
}
