//! Install specs: how a CLI is put on a Windows PC, declared as DATA next to the profile.
//!
//! A code profile ships an `install.toml` beside its `mod.rs` (`include_str!`), a TOML definition
//! adds an `[install.windows]` table. Both parse into the same `InstallSpec`, so the generic install
//! machinery (`crate::agent_install`) never names an agent. A spec holds only closed, validated
//! fields (a winget package id, an npm package name, an https script URL). It never holds a
//! command line: Puppet Master builds the argv itself, so a spec cannot smuggle in shell text.

use serde::{Deserialize, Serialize};

/// The install methods a spec may name, in no particular order.
pub const METHODS: &[&str] = &["winget", "npm", "script"];
/// Tools a spec may say it `requires` (their own install specs live in `agent_install/tools.toml`).
pub const REQUIRED_TOOLS: &[&str] = &["git", "node"];

/// `[install.windows]`: where an agent comes from and which ways exist to install it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallSpec {
    /// The vendor page the information comes from. Shown to the user ("where this comes from").
    pub source: String,
    /// Methods in order of preference (`winget`, `npm`, `script`). Empty = that same order.
    #[serde(default)]
    pub order: Vec<String>,
    /// A winget package id, e.g. `Anthropic.ClaudeCode`.
    pub winget: Option<String>,
    /// An npm package name for `npm install -g`.
    pub npm: Option<String>,
    /// The vendor's own PowerShell installer (`irm <url> | iex`). High risk: shown, and run only
    /// after an extra confirmation, never preferred over winget or npm.
    pub script_url: Option<String>,
    /// Tools every method needs first (`git`, `node`). `npm` needs `node` without saying so.
    #[serde(default)]
    pub requires: Vec<String>,
    /// One plain sentence shown under the command.
    #[serde(default)]
    pub note: String,
}

/// The `[install]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallTable {
    pub windows: Option<InstallSpec>,
}

/// A whole `install.toml` file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallFile {
    #[serde(default)]
    install: InstallTable,
}

/// Parses an `install.toml` document. `Ok(None)` when it declares no Windows install.
pub fn parse_install_toml(text: &str) -> Result<Option<InstallSpec>, String> {
    let file: InstallFile = toml::from_str(text).map_err(|e| format!("install spec: {e}"))?;
    match file.install.windows {
        None => Ok(None),
        Some(spec) => spec.validate().map(|()| Some(spec)),
    }
}

impl InstallSpec {
    /// The methods this spec can run, in preference order, without duplicates.
    pub fn ordered_methods(&self) -> Vec<&'static str> {
        let wanted: Vec<&str> = if self.order.is_empty() {
            METHODS.to_vec()
        } else {
            self.order.iter().map(String::as_str).collect()
        };
        let mut out = Vec::new();
        for name in wanted {
            let Some(method) = METHODS.iter().find(|m| **m == name) else { continue };
            let declared = match *method {
                "winget" => self.winget.is_some(),
                "npm" => self.npm.is_some(),
                _ => self.script_url.is_some(),
            };
            if declared && !out.contains(method) {
                out.push(*method);
            }
        }
        out
    }

    /// Every field is checked against a strict character set, because the values end up in a
    /// command line the user will run.
    pub fn validate(&self) -> Result<(), String> {
        if !is_https_url(&self.source) {
            return Err("install.windows.source must be an https URL".into());
        }
        for method in &self.order {
            if !METHODS.contains(&method.as_str()) {
                return Err(format!("install.windows.order: unknown method {method:?}"));
            }
        }
        if let Some(id) = &self.winget {
            if !is_winget_id(id) {
                return Err(format!("install.windows.winget: {id:?} is not a winget package id"));
            }
        }
        if let Some(package) = &self.npm {
            if !is_npm_package(package) {
                return Err(format!("install.windows.npm: {package:?} is not an npm package name"));
            }
        }
        if let Some(url) = &self.script_url {
            if !is_https_url(url) {
                return Err("install.windows.script_url must be an https URL".into());
            }
        }
        for tool in &self.requires {
            if !REQUIRED_TOOLS.contains(&tool.as_str()) {
                return Err(format!("install.windows.requires: unknown tool {tool:?}"));
            }
        }
        if self.note.chars().count() > 240 || self.note.chars().any(char::is_control) {
            return Err("install.windows.note must be one line of at most 240 characters".into());
        }
        if self.ordered_methods().is_empty() {
            return Err("install.windows declares no usable method (winget, npm or script_url)".into());
        }
        Ok(())
    }
}

/// `Publisher.Package`, letters, digits, dot, dash only (what winget ids are made of).
pub fn is_winget_id(id: &str) -> bool {
    let ok_chars = id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    ok_chars
        && (3..=100).contains(&id.len())
        && id.contains('.')
        && !id.starts_with(['.', '-'])
        && !id.ends_with('.')
        && !id.contains("..")
}

/// A lowercase npm name, optionally scoped. Never starts with `-` (it would read as a flag).
pub fn is_npm_package(name: &str) -> bool {
    if name.is_empty() || name.len() > 214 {
        return false;
    }
    let part = |p: &str| {
        !p.is_empty()
            && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
            && !p.starts_with(['.', '_', '-'])
    };
    match name.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((scope, pkg)) => part(scope) && part(pkg),
            None => false,
        },
        None => part(name),
    }
}

/// `https://host/path?query`, limited to characters that are inert inside a PowerShell
/// single-quoted string and in argv (no quotes, spaces, `;`, `|`, `$`, backticks).
pub fn is_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    url.len() <= 300
        && host.contains('.')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !host.starts_with(['.', '-'])
        && rest.chars().all(|c| c.is_ascii_alphanumeric() || "-._~:/?&=%+@#".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_LIKE: &str = r#"
[install.windows]
source = "https://code.claude.com/docs/en/setup"
order = ["winget", "npm", "script"]
winget = "Anthropic.ClaudeCode"
npm = "@anthropic-ai/claude-code"
script_url = "https://claude.ai/install.ps1"
note = "No Node.js and no administrator needed."
"#;

    #[test]
    fn a_full_spec_parses_and_keeps_its_order() {
        let spec = parse_install_toml(CLAUDE_LIKE).unwrap().unwrap();
        assert_eq!(spec.winget.as_deref(), Some("Anthropic.ClaudeCode"));
        assert_eq!(spec.ordered_methods(), ["winget", "npm", "script"]);
    }

    #[test]
    fn a_file_without_a_windows_table_declares_no_install() {
        assert_eq!(parse_install_toml("").unwrap(), None);
        assert_eq!(parse_install_toml("[install]\n").unwrap(), None);
    }

    #[test]
    fn default_order_prefers_winget_then_npm_then_script() {
        let spec = parse_install_toml(
            "[install.windows]\nsource=\"https://example.com/x\"\nscript_url=\"https://example.com/i.ps1\"\nnpm=\"pkg\"\nwinget=\"A.B\"\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(spec.ordered_methods(), ["winget", "npm", "script"]);
    }

    #[test]
    fn an_order_entry_without_a_value_is_skipped() {
        let spec = parse_install_toml(
            "[install.windows]\nsource=\"https://example.com/x\"\norder=[\"winget\",\"npm\"]\nnpm=\"pkg\"\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(spec.ordered_methods(), ["npm"]);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let bad = "[install.windows]\nsource=\"https://example.com/x\"\nnpm=\"pkg\"\ncommand=\"calc\"\n";
        assert!(parse_install_toml(bad).unwrap_err().contains("command"));
    }

    #[test]
    fn injection_shaped_values_are_rejected() {
        let cases = [
            ("winget", "A.B; calc"),
            ("winget", "A.B' -e"),
            ("winget", "--id"),
            ("winget", "NoDot"),
            ("npm", "pkg && calc"),
            ("npm", "-g"),
            ("npm", "@scope"),
            ("npm", "Pkg"),
            ("script_url", "http://example.com/i.ps1"),
            ("script_url", "https://example.com/i.ps1' | iex; calc; '"),
            ("script_url", "https://exa mple.com/i.ps1"),
            ("script_url", "https://localhost/i.ps1"),
            ("script_url", "https://example.com/$env:TEMP"),
        ];
        for (key, value) in cases {
            let text = format!(
                "[install.windows]\nsource=\"https://example.com/x\"\n{key} = \"{}\"\n",
                value.replace('\\', "\\\\").replace('"', "\\\"")
            );
            // Add a valid fallback so only the bad key can be the reason.
            let text = if key == "winget" { format!("{text}npm=\"ok\"\n") } else { text };
            assert!(parse_install_toml(&text).is_err(), "{key}={value:?} must be rejected");
        }
    }

    #[test]
    fn requires_and_order_only_take_known_names() {
        let base = "[install.windows]\nsource=\"https://example.com/x\"\nnpm=\"pkg\"\n";
        assert!(parse_install_toml(&format!("{base}requires=[\"git\"]")).is_ok());
        assert!(parse_install_toml(&format!("{base}requires=[\"curl\"]")).is_err());
        assert!(parse_install_toml(&format!("{base}order=[\"choco\"]")).is_err());
    }

    #[test]
    fn a_spec_with_no_usable_method_is_rejected() {
        let text = "[install.windows]\nsource=\"https://example.com/x\"\n";
        assert!(parse_install_toml(text).unwrap_err().contains("no usable method"));
    }
}
