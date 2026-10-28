//! Preview origins: which local URLs the preview may open, and the navigation guard (PF-1).
//!
//! PF-2 needs only the origin type and `matches` (the picked page must be the preview's own origin).
//! The rest of this module (`allowed`, `resolve_open`, `nav`, `OwnPorts`) is PF-1's job.

/// The one local origin a preview window shows. Host is normalized: `localhost`, `127.0.0.1` or `[::1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewOrigin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl PreviewOrigin {
    /// `http://localhost:5173`
    pub fn as_str(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    /// True when `url` has this exact scheme, host and port, and no user name or password.
    pub fn matches(&self, url: &str) -> bool {
        let Ok(parsed) = tauri::Url::parse(url) else {
            return false;
        };
        parsed.scheme() == self.scheme
            && parsed.host_str() == Some(self.host.as_str())
            && parsed.port_or_known_default() == Some(self.port)
            && parsed.username().is_empty()
            && parsed.password().is_none()
    }
}
