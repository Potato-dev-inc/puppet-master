//! Discovery of the running app's bridge endpoint (port file lookup and parsing).

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BridgeEndpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl BridgeEndpoint {
    pub(crate) fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

pub(crate) fn read_bridge_endpoint() -> Result<BridgeEndpoint, String> {
    let candidates = bridge_port_candidates();
    read_bridge_endpoint_from_candidates(&candidates)
}

pub(crate) fn read_bridge_endpoint_from_candidates(
    candidates: &[PathBuf],
) -> Result<BridgeEndpoint, String> {
    let mut last_error = String::new();
    for candidate in candidates {
        match fs::read_to_string(candidate) {
            Ok(raw) => {
                let endpoint = parse_bridge_endpoint(&raw)?;
                return Ok(live_endpoint_or_probe(endpoint));
            }
            Err(err) => last_error = format!("{}: {err}", candidate.display()),
        }
    }
    Err(format!(
        "bridge_down: Puppet Master bridge port file not found (tried: {}). Last error: {last_error}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// The port file can be stale (an instance exited and left it behind). When its port refuses
/// connections, fall back to the first port in the bridge range that accepts one.
pub(crate) fn live_endpoint_or_probe(endpoint: BridgeEndpoint) -> BridgeEndpoint {
    let accepts = |host: &str, port: u16| {
        use std::net::{SocketAddr, TcpStream};
        let Ok(ip) = host.parse::<std::net::IpAddr>() else { return true };
        TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(250)).is_ok()
    };
    if accepts(&endpoint.host, endpoint.port) {
        return endpoint;
    }
    for port in 17321..=17330 {
        if accepts(&endpoint.host, port) {
            return BridgeEndpoint { host: endpoint.host, port };
        }
    }
    endpoint
}

pub(crate) fn bridge_port_candidates() -> Vec<PathBuf> {
    if let Ok(path) = env::var(BRIDGE_PORT_FILE_ENV) {
        return vec![PathBuf::from(expand_env_vars(&path, |name| {
            env::var(name).ok()
        }))];
    }
    vec![
        PathBuf::from(DEFAULT_BRIDGE_PORT_FILE),
        default_app_data_bridge_port_file(),
    ]
}

/// Expands `%VAR%` references so config files that cannot expand variables
/// themselves (e.g. an MCP client's `env` block) can still use `%APPDATA%`.
/// Unknown variables are left untouched.
pub(crate) fn expand_env_vars(input: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => match lookup(&after[..end]) {
                Some(value) => {
                    out.push_str(&value);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn default_app_data_bridge_port_file() -> PathBuf {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    } else if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    } else {
        home.join(".local")
            .join("share")
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    }
}

pub(crate) fn parse_bridge_endpoint(raw: &str) -> Result<BridgeEndpoint, String> {
    let trimmed = raw.trim();
    let (host, port_text) = trimmed
        .split_once(':')
        .map(|(host, port)| (if host.is_empty() { "127.0.0.1" } else { host }, port))
        .unwrap_or(("127.0.0.1", trimmed));
    let port = port_text
        .parse::<u16>()
        .map_err(|err| format!("invalid bridge port file value {trimmed:?}: {err}"))?;
    Ok(BridgeEndpoint {
        host: host.to_string(),
        port,
    })
}

#[cfg(test)]
mod env_expansion_tests {
    use super::expand_env_vars;

    fn lookup(name: &str) -> Option<String> {
        (name == "APPDATA").then(|| r"C:\Users\x\AppData\Roaming".to_string())
    }

    #[test]
    fn expands_known_variable() {
        assert_eq!(
            expand_env_vars(r"%APPDATA%\app\bridge.port", lookup),
            r"C:\Users\x\AppData\Roaming\app\bridge.port"
        );
    }

    #[test]
    fn leaves_unknown_and_unpaired_percent_untouched() {
        assert_eq!(expand_env_vars("%NOPE%/a", lookup), "%NOPE%/a");
        assert_eq!(expand_env_vars("100%/a", lookup), "100%/a");
        assert_eq!(expand_env_vars("%%", lookup), "%%");
    }
}
