//! A project's identity across machines: its normalised `origin` remote. Maya's clone and Leo's
//! clone of the same repo get the same key whatever URL form each used.
//!
//! - `git@github.com:acme/x.git`, `https://github.com/acme/x`, `ssh://git@github.com/acme/x.git`
//!   → `github.com/acme/x`
//! - user names and passwords in the URL are dropped (they must never leave the machine)
//! - a local path (`D:\team\origin.git`, `file:///d/team/origin.git`) → `file:d:/team/origin`

use std::path::Path;

/// The normalised key of a remote URL, or `None` when it is empty.
pub fn normalize_remote(url: &str) -> Option<String> {
    let raw = url.trim();
    if raw.is_empty() {
        return None;
    }
    let strip_git = |s: &str| -> String {
        let s = s.trim_end_matches('/');
        s.strip_suffix(".git").unwrap_or(s).trim_end_matches('/').to_string()
    };
    if let Some(rest) = raw.strip_prefix("file://") {
        // file:///d:/x or file:///D/x or file://localhost/x
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        let rest = rest.trim_start_matches('/');
        return Some(format!("file:{}", local_key(&strip_git(rest))));
    }
    if let Some((scheme, rest)) = raw.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if matches!(scheme.as_str(), "http" | "https" | "ssh" | "git" | "git+ssh" | "ssh+git") {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = authority.rsplit('@').next().unwrap_or(authority);
            // ssh://host:22/path: the port is not part of the identity.
            let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
            let path = strip_git(path.trim_start_matches('/'));
            return Some(join_host(&host, &path));
        }
    }
    // scp-like: [user@]host:path (not a Windows drive like C:\ or C:/)
    if let Some((left, right)) = raw.split_once(':') {
        let is_drive = left.len() == 1 && left.chars().all(|c| c.is_ascii_alphabetic());
        if !is_drive && !left.contains('/') && !left.contains('\\') {
            let host = left.rsplit('@').next().unwrap_or(left).to_ascii_lowercase();
            let path = strip_git(right.trim_start_matches('/'));
            return Some(join_host(&host, &path));
        }
    }
    Some(format!("file:{}", local_key(&strip_git(raw))))
}

fn join_host(host: &str, path: &str) -> String {
    if path.is_empty() {
        host.to_string()
    } else {
        format!("{host}/{path}")
    }
}

/// Forward slashes, no trailing slash; case-folded on Windows, where paths are case-insensitive.
fn local_key(path: &str) -> String {
    let mut text = path.replace('\\', "/");
    while text.ends_with('/') {
        text.pop();
    }
    if cfg!(windows) {
        text = text.to_ascii_lowercase();
        // `/d/team/x` (MSYS) and `d:/team/x` are the same folder.
        let bytes = text.as_bytes();
        if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b'/' {
            text = format!("{}:{}", bytes[1] as char, &text[2..]);
        }
    }
    text
}

/// Last segment of a key, for display (`github.com/acme/billing-service` → `billing-service`).
pub fn display_name(key: &str) -> String {
    key.trim_start_matches("file:")
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(key)
        .to_string()
}

/// The `origin` key of the repository at `dir`, read with the git CLI (no network).
pub fn project_key_of(dir: &Path) -> Option<String> {
    let url = super::git::run(dir, &["config", "--get", "remote.origin.url"]).ok()?;
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    // A relative local remote (`../origin.git`) is relative to the repository.
    let looks_relative = url.starts_with("./") || url.starts_with("../") || url.starts_with(".\\") || url.starts_with("..\\");
    if looks_relative {
        let joined = dir.join(url);
        let full = std::fs::canonicalize(&joined).unwrap_or(joined);
        return normalize_remote(&crate::app_paths::path_for_host_config(full));
    }
    normalize_remote(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_urls_of_one_repo_share_a_key() {
        let want = Some("github.com/acme/billing-service".to_string());
        for url in [
            "git@github.com:acme/billing-service.git",
            "https://github.com/acme/billing-service",
            "https://github.com/acme/billing-service.git/",
            "ssh://git@github.com/acme/billing-service.git",
            "ssh://git@GitHub.com:22/acme/billing-service.git",
            "https://maya:ghp_secret@github.com/acme/billing-service.git",
        ] {
            assert_eq!(normalize_remote(url), want, "{url}");
        }
    }

    #[test]
    fn credentials_never_survive_normalisation() {
        let key = normalize_remote("https://user:hunter2@gitlab.example.com/team/app.git").unwrap();
        assert_eq!(key, "gitlab.example.com/team/app");
        assert!(!key.contains("hunter2") && !key.contains("user"));
    }

    #[test]
    fn local_paths_normalise() {
        let a = normalize_remote(r"D:\pm-team-demo\origin.git").unwrap();
        let b = normalize_remote("D:/pm-team-demo/origin.git").unwrap();
        assert_eq!(a, b);
        assert!(a.starts_with("file:"));
        if cfg!(windows) {
            assert_eq!(a, "file:d:/pm-team-demo/origin");
            assert_eq!(normalize_remote("file:///D:/pm-team-demo/origin.git").unwrap(), a);
            assert_eq!(normalize_remote("/d/pm-team-demo/origin.git").unwrap(), a);
        }
        assert_eq!(display_name(&a), "origin");
        assert_eq!(display_name("github.com/acme/billing-service"), "billing-service");
        assert_eq!(normalize_remote("  "), None);
    }
}
