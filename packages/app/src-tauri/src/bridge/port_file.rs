//! The bridge port file (`puppet-master.bridge.port`) is how MCP clients find the running app.
//! Only the process that owns the bridge writes it, and on exit it removes the file only if the
//! file still names this process's port (a later instance may have replaced it).

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU16, Ordering};

static OWNED_PORT: AtomicU16 = AtomicU16::new(0);

pub fn port_line(host: &str, port: u16) -> String {
    format!("{host}:{port}\n")
}

/// Write `host:port` and remember the port as ours.
pub fn write_owned(path: &Path, host: &str, port: u16) -> std::io::Result<()> {
    fs::write(path, port_line(host, port))?;
    OWNED_PORT.store(port, Ordering::SeqCst);
    Ok(())
}

/// Port named by the file contents (`host:port`), if parseable.
pub fn parse_port(raw: &str) -> Option<u16> {
    raw.trim().rsplit(':').next()?.trim().parse().ok()
}

/// Remove the file only when it still names `port`. Returns true when removed.
pub fn release_if_owned(path: &Path, port: u16) -> bool {
    match fs::read_to_string(path) {
        Ok(raw) if parse_port(&raw) == Some(port) => fs::remove_file(path).is_ok(),
        _ => false,
    }
}

/// Exit hook: release the file if this process wrote it.
pub fn release_owned(path: &Path) -> bool {
    let port = OWNED_PORT.swap(0, Ordering::SeqCst);
    port != 0 && release_if_owned(path, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pm-port-{name}-{}", std::process::id()))
    }

    #[test]
    fn parses_host_port() {
        assert_eq!(parse_port("127.0.0.1:17321\n"), Some(17321));
        assert_eq!(parse_port("junk"), None);
    }

    #[test]
    fn removes_only_when_it_still_names_our_port() {
        let path = temp_file("own");
        fs::write(&path, port_line("127.0.0.1", 17321)).unwrap();
        assert!(!release_if_owned(&path, 17322), "another instance's port must stay");
        assert!(path.exists());
        assert!(release_if_owned(&path, 17321));
        assert!(!path.exists());
        assert!(!release_if_owned(&path, 17321), "missing file is a no-op");
    }
}
