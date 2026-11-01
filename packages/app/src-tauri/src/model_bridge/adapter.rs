//! Adapters: one TOML file per model service. Embedded defaults plus the person's own folder
//! (`<app data>/model_bridge/adapters/*.toml`; a file there with the same `id` replaces the default).
//! Generic code reads only this data and names no provider.
//!
//! An adapter never holds a key. The key lives in the OS credential store under `key_slot`.

use serde::{Deserialize, Serialize};
use std::path::Path;

const EMBEDDED: &[(&str, &str)] = &[
    ("opencode-go.toml", include_str!("adapters/opencode-go.toml")),
    ("any-responses.toml", include_str!("adapters/any-responses.toml")),
];

/// Which wire API a model family speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Responses,
    ChatCompletions,
    AnthropicMessages,
    Gemini,
}

impl Shape {
    pub fn as_str(self) -> &'static str {
        match self {
            Shape::Responses => "responses",
            Shape::ChatCompletions => "chat_completions",
            Shape::AnthropicMessages => "anthropic_messages",
            Shape::Gemini => "gemini",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auth {
    pub header: String,
    pub scheme: String,
    /// Name in the credential store. Defaults to `model_bridge:<id>`.
    #[serde(default)]
    pub key_slot: Option<String>,
    /// The environment variable an app reads the key from.
    pub env_var: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub shape: Shape,
    /// Overrides the adapter's `base_url` for this route.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adapter {
    pub id: String,
    pub display_name: String,
    /// Empty only for the blank adapter (`custom = true`) the person fills in.
    pub base_url: String,
    #[serde(default)]
    pub needs_review: bool,
    /// The blank adapter: the person types the name and the address.
    #[serde(default)]
    pub custom: bool,
    #[serde(default)]
    pub note: Option<String>,
    pub auth: Auth,
    pub routes: Vec<Route>,
    #[serde(default)]
    pub limits: Limits,
}

impl Adapter {
    /// The credential-store name for this adapter's key.
    pub fn key_slot(&self) -> String {
        self.auth.key_slot.clone().unwrap_or_else(|| format!("model_bridge:{}", self.id))
    }

    pub fn route(&self, shape: Shape) -> Option<&Route> {
        self.routes.iter().find(|r| r.shape == shape)
    }

    /// The address for a route: its own, else the adapter's.
    pub fn base_url_for<'a>(&'a self, route: &'a Route) -> &'a str {
        route.base_url.as_deref().unwrap_or(&self.base_url)
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Whether `url` is an address a key may be sent to: https, or http only to this computer.
pub fn valid_service_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let host_only = host.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host);
    match scheme {
        "https" => true,
        "http" => matches!(host_only, "localhost" | "127.0.0.1" | "[::1]"),
        _ => false,
    }
}

/// Checks an adapter. The error is plain words and names the file's problem, never a key.
pub fn validate(a: &Adapter) -> Result<(), String> {
    if !valid_id(&a.id) {
        return Err("The id may use only lower-case letters, digits, - and _ (up to 40).".into());
    }
    if a.display_name.trim().is_empty() || a.display_name.len() > 80 {
        return Err("The name is missing or too long.".into());
    }
    if a.auth.header.trim().is_empty() || a.auth.header.chars().any(|c| c.is_whitespace() || c == ':') {
        return Err("The key header is not a valid header name.".into());
    }
    if !valid_env_name(&a.auth.env_var) {
        return Err("The variable name must be UPPER_CASE letters, digits and _.".into());
    }
    if let Some(slot) = &a.auth.key_slot {
        if slot.is_empty() || slot.len() > 80 || !slot.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':')) {
            return Err("The key slot name is not valid.".into());
        }
    }
    if a.routes.is_empty() {
        return Err("The service lists no way to talk to it.".into());
    }
    if !a.custom {
        if !valid_service_url(&a.base_url) {
            return Err("The address must start with https:// (or http:// for this computer).".into());
        }
        for r in &a.routes {
            if let Some(url) = &r.base_url {
                if !valid_service_url(url) {
                    return Err("A route address must start with https:// (or http:// for this computer).".into());
                }
            }
        }
    }
    Ok(())
}

/// Parses and checks one adapter file.
pub fn parse(text: &str) -> Result<Adapter, String> {
    let adapter: Adapter = toml::from_str(text).map_err(|e| format!("Could not read the adapter file: {}", e.message()))?;
    validate(&adapter)?;
    Ok(adapter)
}

#[derive(Debug, Default)]
pub struct Loaded {
    pub adapters: Vec<Adapter>,
    /// One plain line per user file that could not be used. The defaults still load.
    pub problems: Vec<String>,
}

/// The embedded defaults, then the files in `user_dir` (a file with a known id replaces that default).
pub fn load_all(user_dir: Option<&Path>) -> Loaded {
    let mut out = Loaded::default();
    for (name, text) in EMBEDDED {
        match parse(text) {
            Ok(a) => out.adapters.push(a),
            Err(e) => out.problems.push(format!("Built-in adapter {name}: {e}")),
        }
    }
    let Some(dir) = user_dir else { return out };
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    files.sort();
    for path in files {
        let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) if t.len() <= 64 * 1024 => t,
            Ok(_) => {
                out.problems.push(format!("{label}: the file is too big."));
                continue;
            }
            Err(_) => {
                out.problems.push(format!("{label}: the file could not be read."));
                continue;
            }
        };
        match parse(&text) {
            Ok(a) => match out.adapters.iter().position(|x| x.id == a.id) {
                Some(i) => out.adapters[i] = a,
                None => out.adapters.push(a),
            },
            Err(e) => out.problems.push(format!("{label}: {e}")),
        }
    }
    out
}

/// `My Service!` becomes `my-service`. Empty when nothing usable is left.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(30).collect()
}

/// Builds the person's own adapter from the blank one: a name and an address. Responses route only.
pub fn custom_from(template: &Adapter, name: &str, url: &str) -> Result<Adapter, String> {
    let name = name.trim();
    let url = url.trim().trim_end_matches('/');
    let id_part = slug(name);
    if name.is_empty() || id_part.is_empty() {
        return Err("Type a name for the service.".into());
    }
    if !valid_service_url(url) {
        return Err("The address must start with https:// (or http:// for this computer).".into());
    }
    let id = format!("custom-{id_part}");
    let env_var = format!("PM_MODEL_KEY_{}", id_part.to_uppercase().replace('-', "_"));
    let adapter = Adapter {
        id,
        display_name: name.to_string(),
        base_url: url.to_string(),
        needs_review: false,
        custom: false,
        note: None,
        auth: Auth { env_var, ..template.auth.clone() },
        routes: vec![Route { shape: Shape::Responses, base_url: None, models: Vec::new() }],
        limits: Limits::default(),
    };
    validate(&adapter)?;
    Ok(adapter)
}

/// The TOML text of an adapter, for the user folder.
pub fn to_toml(a: &Adapter) -> Result<String, String> {
    toml::to_string_pretty(a).map_err(|e| format!("Could not write the adapter: {e}"))
}
