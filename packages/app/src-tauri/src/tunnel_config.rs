//! Bring-your-own tunnel or domain: the persisted tunnel setting and the pure logic around it.
//!
//! Why a separate file: the named-tunnel token is a secret. The public settings mirror
//! (`settings_store.rs`) is broadcast over SSE to every paired phone and only whitelists
//! `PATCHABLE_KEYS`, so the tunnel config is owned by Rust, lives in its own file in the app data
//! dir (`puppet-master.tunnel.json`) and is only ever returned to the UI in a redacted view.
//!
//! Modes:
//! - `quick`  (default): bundled cloudflared quick tunnel; the trycloudflare URL changes per launch.
//! - `named`: `cloudflared tunnel run` with a Cloudflare tunnel token; `hostname` is the stable origin.
//! - `custom`: the user runs their own proxy or domain; Puppet Master just advertises the URL.
//!
//! `PUPPET_MASTER_PUBLIC_URL` (env) still wins over everything and skips cloudflared.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const TUNNEL_FILE_NAME: &str = "puppet-master.tunnel.json";
pub const TUNNEL_TOKEN_ENV: &str = "TUNNEL_TOKEN";

pub fn tunnel_file() -> std::path::PathBuf {
    crate::app_paths::app_data_dir().join(TUNNEL_FILE_NAME)
}

/// The persisted setting. The token never leaves Rust except as the `TUNNEL_TOKEN` env var of the
/// cloudflared child process.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TunnelConfig {
    #[default]
    Quick,
    Named {
        token: String,
        hostname: String,
    },
    Custom {
        url: String,
    },
}

// Manual Debug so a stray `{:?}` can never print the token.
impl std::fmt::Debug for TunnelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quick => write!(f, "Quick"),
            Self::Named { hostname, .. } => write!(f, "Named {{ hostname: {hostname:?}, token: <redacted> }}"),
            Self::Custom { url } => write!(f, "Custom {{ url: {url:?} }}"),
        }
    }
}

/// What the UI is allowed to see: never the token, only whether one is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TunnelConfigView {
    pub mode: &'static str,
    pub hostname: Option<String>,
    pub url: Option<String>,
    pub has_token: bool,
}

/// What the UI sends. `token: None` on a `named` save means "keep the stored token".
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TunnelConfigInput {
    Quick,
    Named {
        #[serde(default)]
        token: Option<String>,
        hostname: String,
    },
    Custom {
        url: String,
    },
}

impl TunnelConfig {
    pub fn view(&self) -> TunnelConfigView {
        match self {
            Self::Quick => TunnelConfigView { mode: "quick", hostname: None, url: None, has_token: false },
            Self::Named { token, hostname } => TunnelConfigView {
                mode: "named",
                hostname: Some(hostname.clone()),
                url: None,
                has_token: !token.is_empty(),
            },
            Self::Custom { url } => TunnelConfigView { mode: "custom", hostname: None, url: Some(url.clone()), has_token: false },
        }
    }
}

/// Validate UI input into a stored config. `current` supplies the stored token when the UI omits it.
pub fn config_from_input(input: TunnelConfigInput, current: &TunnelConfig) -> Result<TunnelConfig, String> {
    match input {
        TunnelConfigInput::Quick => Ok(TunnelConfig::Quick),
        TunnelConfigInput::Custom { url } => Ok(TunnelConfig::Custom { url: normalize_origin(&url, false)? }),
        TunnelConfigInput::Named { token, hostname } => {
            let origin = normalize_origin(&hostname, true)?;
            let stored = match current {
                TunnelConfig::Named { token, .. } => token.as_str(),
                _ => "",
            };
            let token = match token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
                Some(t) => t,
                None if !stored.is_empty() => stored.to_string(),
                None => return Err("Paste the tunnel token from Cloudflare.".into()),
            };
            if token.chars().any(char::is_whitespace) {
                return Err("The tunnel token must not contain spaces or line breaks.".into());
            }
            // Store just the host; the origin is derived (always https for a named tunnel).
            let hostname = origin.trim_start_matches("https://").to_string();
            Ok(TunnelConfig::Named { token, hostname })
        }
    }
}

/// Turn what a person typed (`example.com`, `https://example.com/`, `https://x.dev/bridge`) into a
/// clean origin with no path. `force_https`: a named tunnel is always served over https.
pub fn normalize_origin(raw: &str, force_https: bool) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter a domain, for example pm.example.com.".into());
    }
    let (scheme, rest) = match trimmed.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => ("https".to_string(), trimmed),
    };
    if scheme != "https" && scheme != "http" {
        return Err("Use an http:// or https:// address.".into());
    }
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.is_empty() || host_port.contains('@') || host_port.chars().any(char::is_whitespace) {
        return Err("That does not look like a domain.".into());
    }
    let host = host_port.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host_port }).unwrap_or(host_port);
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err("That does not look like a domain.".into());
    }
    if force_https && !host.contains('.') {
        return Err("Enter the full hostname, for example pm.example.com.".into());
    }
    let scheme = if force_https { "https" } else { scheme.as_str() };
    Ok(format!("{scheme}://{}", host_port.to_ascii_lowercase()))
}

pub fn load(path: &Path) -> TunnelConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<TunnelConfig>(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, config: &TunnelConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|err| err.to_string())?;
    // Write to a temp file then rename so a crash never leaves a half-written token file.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|err| format!("write {}: {err}", tmp.display()))?;
    restrict_permissions(&tmp);
    fs::rename(&tmp, path).map_err(|err| format!("replace {}: {err}", path.display()))
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {} // %APPDATA% is already per-user.

/// Which tunnel actually runs, after the env override is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelPlan {
    /// `PUPPET_MASTER_PUBLIC_URL` is set: no cloudflared.
    Env { url: String },
    Quick,
    Named { origin: String },
    Custom { url: String },
}

impl TunnelPlan {
    pub fn mode(&self) -> &'static str {
        match self {
            Self::Env { .. } => "env",
            Self::Quick => "quick",
            Self::Named { .. } => "named",
            Self::Custom { .. } => "custom",
        }
    }

    /// The URL known at plan time (quick learns its URL later from cloudflared's output).
    pub fn known_public_url(&self) -> Option<String> {
        match self {
            Self::Env { url } | Self::Custom { url } => Some(url.clone()),
            Self::Named { origin } => Some(origin.clone()),
            Self::Quick => None,
        }
    }

    pub fn runs_cloudflared(&self) -> bool {
        matches!(self, Self::Quick | Self::Named { .. })
    }
}

pub fn resolve_plan(env_public_url: Option<String>, config: &TunnelConfig) -> TunnelPlan {
    if let Some(url) = env_public_url {
        return TunnelPlan::Env { url };
    }
    match config {
        TunnelConfig::Quick => TunnelPlan::Quick,
        TunnelConfig::Named { hostname, .. } => TunnelPlan::Named { origin: format!("https://{hostname}") },
        TunnelConfig::Custom { url } => TunnelPlan::Custom { url: url.clone() },
    }
}

/// cloudflared arguments. The named token is passed through the `TUNNEL_TOKEN` env var instead of
/// `--token <T>` so it does not show up in the process list.
pub fn cloudflared_args(plan: &TunnelPlan, local_target: &str) -> Vec<String> {
    match plan {
        TunnelPlan::Named { .. } => vec!["tunnel".into(), "--no-autoupdate".into(), "run".into()],
        _ => vec!["tunnel".into(), "--url".into(), local_target.into()],
    }
}

/// Where "Test connection" goes: the public origin's bridge health route, which is public (no
/// bearer needed) so it can be probed before any phone is paired.
pub fn health_url(public_origin: &str) -> String {
    format!("{}/bridge/health", public_origin.trim_end_matches('/'))
}

/// A cloudflared log line, reduced to what the status needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedLine {
    Connected,
    Failed(String),
    Other,
}

pub fn classify_named_line(line: &str) -> NamedLine {
    if line.contains("Registered tunnel connection") {
        return NamedLine::Connected;
    }
    let lower = line.to_ascii_lowercase();
    if lower.contains("unauthorized") || lower.contains("invalid tunnel secret") || lower.contains("tunnel token") && lower.contains("err") {
        return NamedLine::Failed("Cloudflare rejected the tunnel token.".into());
    }
    if lower.contains("error parsing tunnel id") || lower.contains("provided tunnel token is not valid") {
        return NamedLine::Failed("The tunnel token is not valid.".into());
    }
    NamedLine::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(token: &str, host: &str) -> TunnelConfig {
        TunnelConfig::Named { token: token.into(), hostname: host.into() }
    }

    #[test]
    fn default_is_quick_and_missing_file_loads_default() {
        assert_eq!(TunnelConfig::default(), TunnelConfig::Quick);
        assert_eq!(load(Path::new("Z:/definitely/not/here.json")), TunnelConfig::Quick);
    }

    #[test]
    fn roundtrip_persists_in_own_file_and_view_never_contains_token() {
        let dir = std::env::temp_dir().join(format!("pm-tunnel-test-{}", std::process::id()));
        let path = dir.join(TUNNEL_FILE_NAME);
        let cfg = named("eyJhIjoiU0VDUkVU", "pm.example.com");
        save(&path, &cfg).unwrap();
        assert_eq!(load(&path), cfg);
        let view = serde_json::to_string(&cfg.view()).unwrap();
        assert!(!view.contains("eyJhIjoiU0VDUkVU"), "view leaked the token: {view}");
        assert!(view.contains("\"has_token\":true"));
        assert!(!format!("{cfg:?}").contains("eyJhIjoiU0VDUkVU"), "Debug leaked the token");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn token_is_not_in_the_public_settings_mirror() {
        // The mirror only copies PATCHABLE_KEYS; a tunnel config pushed at it is dropped.
        let patch = serde_json::json!({"mode": "named", "token": "SECRET", "hostname": "x.example.com"});
        let merged = crate::settings_store::merge_public_settings(&serde_json::json!({}), &patch);
        assert!(!merged.to_string().contains("SECRET"));
    }

    #[test]
    fn normalize_origin_cleans_what_people_type() {
        assert_eq!(normalize_origin("PM.Example.com", true).unwrap(), "https://pm.example.com");
        assert_eq!(normalize_origin("https://pm.example.com/", false).unwrap(), "https://pm.example.com");
        assert_eq!(normalize_origin("https://pm.example.com/bridge/health", false).unwrap(), "https://pm.example.com");
        assert_eq!(normalize_origin("http://192.168.1.5:8080/x", false).unwrap(), "http://192.168.1.5:8080");
        assert_eq!(normalize_origin("http://pm.example.com", true).unwrap(), "https://pm.example.com");
        assert!(normalize_origin("", false).is_err());
        assert!(normalize_origin("ftp://x.com", false).is_err());
        assert!(normalize_origin("https://user@evil.com", false).is_err());
        assert!(normalize_origin("localhost", true).is_err());
        assert!(normalize_origin("bad host.com", false).is_err());
    }

    #[test]
    fn named_input_keeps_stored_token_when_omitted_and_requires_one_otherwise() {
        let current = named("TOK", "old.example.com");
        let kept = config_from_input(TunnelConfigInput::Named { token: None, hostname: "new.example.com".into() }, &current).unwrap();
        assert_eq!(kept, named("TOK", "new.example.com"));
        let replaced = config_from_input(TunnelConfigInput::Named { token: Some(" NEW ".into()), hostname: "a.example.com".into() }, &current).unwrap();
        assert_eq!(replaced, named("NEW", "a.example.com"));
        assert!(config_from_input(TunnelConfigInput::Named { token: None, hostname: "a.example.com".into() }, &TunnelConfig::Quick).is_err());
        assert!(config_from_input(TunnelConfigInput::Named { token: Some("a b".into()), hostname: "a.example.com".into() }, &TunnelConfig::Quick).is_err());
    }

    #[test]
    fn plan_env_wins_then_config() {
        let cfg = named("T", "pm.example.com");
        assert_eq!(resolve_plan(Some("https://env.dev".into()), &cfg), TunnelPlan::Env { url: "https://env.dev".into() });
        assert_eq!(resolve_plan(None, &cfg), TunnelPlan::Named { origin: "https://pm.example.com".into() });
        assert_eq!(resolve_plan(None, &TunnelConfig::Quick), TunnelPlan::Quick);
        let custom = TunnelConfig::Custom { url: "https://my.dev".into() };
        let plan = resolve_plan(None, &custom);
        assert_eq!(plan.known_public_url().as_deref(), Some("https://my.dev"));
        assert!(!plan.runs_cloudflared());
        assert!(TunnelPlan::Quick.runs_cloudflared());
    }

    #[test]
    fn cloudflared_args_never_contain_the_token() {
        let plan = TunnelPlan::Named { origin: "https://pm.example.com".into() };
        let args = cloudflared_args(&plan, "http://127.0.0.1:1420");
        assert_eq!(args, ["tunnel", "--no-autoupdate", "run"]);
        assert_eq!(cloudflared_args(&TunnelPlan::Quick, "http://127.0.0.1:1420"), ["tunnel", "--url", "http://127.0.0.1:1420"]);
    }

    #[test]
    fn health_url_targets_bridge_health() {
        assert_eq!(health_url("https://pm.example.com/"), "https://pm.example.com/bridge/health");
    }

    #[test]
    fn named_line_classification() {
        assert_eq!(classify_named_line("INF Registered tunnel connection connIndex=0"), NamedLine::Connected);
        assert!(matches!(classify_named_line("ERR Unauthorized: invalid tunnel secret"), NamedLine::Failed(_)));
        assert!(matches!(classify_named_line("Provided tunnel token is not valid"), NamedLine::Failed(_)));
        assert_eq!(classify_named_line("INF Starting tunnel"), NamedLine::Other);
    }
}
