//! Strict redaction for everything the recorder keeps (REC-3). Raw text never reaches a snapshot:
//! `redact` runs before a capture is stored, so neither memory nor disk holds an unredacted copy.
//! It errs toward over-redaction: a lost word in a draft costs a human one edit, a leaked key costs more.

use once_cell::sync::Lazy;
use regex::Regex;

pub const MASK: &str = "[REDACTED]";

/// What this process knows to be secret or personal.
#[derive(Debug, Clone, Default)]
pub struct RedactContext {
    /// Values of environment variables worth masking (see `secret_env_values`).
    pub env_values: Vec<String>,
    /// The user's home directory, any slash style.
    pub home: Option<String>,
}

impl RedactContext {
    pub fn from_process() -> Self {
        let env = std::env::vars().collect::<Vec<_>>();
        let home = crate::project_path::home_dir().map(|p| p.to_string_lossy().to_string());
        Self { env_values: secret_env_values(&env), home }
    }
}

/// Values of secret-named variables (any length >= 4) and of other variables that look like an opaque
/// token (>= 12 chars, no whitespace or path/list punctuation, so `PATH`-style values stay readable).
pub fn secret_env_values(env: &[(String, String)]) -> Vec<String> {
    const SECRET_NAMES: [&str; 8] = ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "PASS", "CREDENTIAL", "AUTH"];
    let mut out: Vec<String> = Vec::new();
    for (name, value) in env {
        let upper = name.to_ascii_uppercase();
        let secret_named = SECRET_NAMES.iter().any(|s| upper.contains(s));
        let opaque = value.len() >= 12
            && !value.chars().any(|c| c.is_whitespace() || matches!(c, '\\' | '/' | ';' | ':' | ','));
        if (secret_named && value.len() >= 4) || opaque {
            out.push(value.clone());
        }
    }
    out.sort_by_key(|v| std::cmp::Reverse(v.len()));
    out.dedup();
    out
}

static PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"sk-[A-Za-z0-9_\-]{6,}",                   // OpenAI/Anthropic style (sk-ant-..., sk-proj-...)
        r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{6,}", // GitHub tokens
        r"github_pat_[A-Za-z0-9_]{6,}",
        r"(?:AKIA|ASIA)[0-9A-Z]{12,}", // AWS access key ids
        r"xox[abprs]-[A-Za-z0-9\-]{6,}", // Slack
        r"eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{4,}\.[A-Za-z0-9_\-]{0,}", // JWT
        r"(?i)bearer\s+[A-Za-z0-9._~+/=\-]{8,}",
        r"(?i)(?:api[_-]?key|token|secret|password|passwd)\s*[=:]\s*\S{4,}",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("redaction pattern"))
    .collect()
});

/// A long opaque run: 40+ base64/hex/url-safe characters (at least 30 of them letters or digits, so
/// rules drawn with `=` or `-` survive).
static LONG_RUN: Lazy<Regex> = Lazy::new(|| Regex::new(r"[A-Za-z0-9+/=_\-]{40,}").expect("long run"));

static USER_DIR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)([A-Za-z]:[\\/]+users[\\/]+|/home/|/users/)[^\\/\s]+").expect("user dir"));

fn replace_ci(text: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    match Regex::new(&format!("(?i){}", regex::escape(needle))) {
        Ok(re) => re.replace_all(text, regex::NoExpand(with)).into_owned(),
        Err(_) => text.to_string(),
    }
}

/// Mask secrets and personal paths in `text`.
pub fn redact(text: &str, ctx: &RedactContext) -> String {
    let mut out = text.to_string();
    for value in &ctx.env_values {
        if value.len() >= 4 {
            out = replace_ci(&out, value, MASK);
        }
    }
    for re in PATTERNS.iter() {
        out = re.replace_all(&out, MASK).into_owned();
    }
    out = LONG_RUN
        .replace_all(&out, |caps: &regex::Captures| {
            let run = &caps[0];
            if run.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 30 {
                MASK.to_string()
            } else {
                run.to_string()
            }
        })
        .into_owned();
    if let Some(home) = ctx.home.as_deref().filter(|h| h.len() >= 3) {
        let trimmed = home.trim_end_matches(['\\', '/']);
        out = replace_ci(&out, trimmed, "~");
        out = replace_ci(&out, &trimmed.replace('\\', "/"), "~");
    }
    // Any other user's folder (or a different spelling of this one) loses its name.
    USER_DIR.replace_all(&out, "${1}USER").into_owned()
}

#[cfg(test)]
pub(super) fn test_ctx() -> RedactContext {
    RedactContext { env_values: vec!["hunter2-secret".into()], home: Some(r"C:\Users\Ren-pc".into()) }
}
