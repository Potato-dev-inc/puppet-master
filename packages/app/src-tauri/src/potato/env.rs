//! The brain's environment (spec section 6.3). It is an allowlist: the process starts from
//! `env_clear()` and gets only the names below, nothing inherited.
//!
//! - always: `PATH`, `SystemRoot`, `WINDIR`, `TEMP`, `TMP`;
//! - the home variables, only for the Go plan brain (it reads the person's sign-in file);
//! - the brain's own settings: `FISHD_PROVIDER`, `FISHD_MODEL`, `FISHD_VAULT`, `FISHD_PERSONA_DIR`,
//!   `FISHD_STATE_DIR`, `FISHD_PACK`, plus the fixed `FISHD_VOICE=0`, `FISHD_GATE=heuristic` and
//!   `FISHD_SHARE_DIR` (an empty folder);
//! - `DEEPSEEK_API_KEY`, only for the DeepSeek brain, and only in the environment: never an argument;
//! - `PM_POTATO_URL`, `PM_POTATO_TOKEN`, `PM_POTATO_BOT`.
//!
//! Never in it: any `PUPPET_MASTER_*` variable (so no port file and no local secret reach the brain),
//! any other secret. `Debug` prints names only. Pure: the caller passes in what it read from the host.

use std::fmt;
use std::path::Path;
use std::process::Command;

use super::wire_names;

/// The names the host contributes, in the form the host spells them.
pub const HOST_NAMES: [&str; 5] = ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP"];
/// Where the Go plan sign-in file is looked for.
pub const HOME_NAMES: [&str; 5] = ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_DATA_HOME"];
pub const KEY_NAME: &str = "DEEPSEEK_API_KEY";

/// Which model the brain talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brain {
    /// Pays the model maker per message, with the person's own key.
    DeepSeek,
    /// Uses the person's Go plan sign-in. Needs the home variables.
    Go,
}

/// What the host process has, read once by the caller.
#[derive(Debug, Clone, Default)]
pub struct HostEnv {
    pub values: Vec<(String, String)>,
}

impl HostEnv {
    /// `PATH` is passed in (the app's own discovered path), the rest is read from this process.
    pub fn from_process(path: &str) -> HostEnv {
        let mut values = vec![("PATH".to_string(), path.to_string())];
        for name in HOST_NAMES.iter().skip(1).chain(HOME_NAMES.iter()) {
            if let Some(value) = std::env::var_os(name).and_then(|v| v.into_string().ok()).filter(|v| !v.is_empty()) {
                values.push(((*name).to_string(), value));
            }
        }
        HostEnv { values }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// Everything the environment is made from.
pub struct Inputs<'a> {
    pub brain: Brain,
    pub pack: &'a str,
    pub model: Option<&'a str>,
    pub vault: &'a Path,
    pub persona_dir: &'a Path,
    pub state_dir: &'a Path,
    /// An empty folder, so the brain has nothing to share.
    pub share_dir: &'a Path,
    pub url: &'a str,
    pub token: &'a str,
    pub bot: &'a str,
    /// Read from the credential store by the caller; used only for `Brain::DeepSeek`.
    pub deepseek_key: Option<&'a str>,
    pub host: &'a HostEnv,
}

/// The built environment: names and values. `Debug` shows the names and never a value.
#[derive(Clone)]
pub struct BrainEnv {
    vars: Vec<(String, String)>,
}

impl fmt::Debug for BrainEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrainEnv").field("names", &self.names()).finish()
    }
}

impl BrainEnv {
    pub fn names(&self) -> Vec<&str> {
        self.vars.iter().map(|(k, _)| k.as_str()).collect()
    }

    #[cfg(test)]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The exact secret values in it, for the log cleaner.
    pub fn secrets(&self) -> Vec<String> {
        self.vars.iter().filter(|(k, _)| k == KEY_NAME || k == "PM_POTATO_TOKEN").map(|(_, v)| v.clone()).collect()
    }

    /// Clears the command's environment and sets exactly this one.
    pub fn apply(&self, command: &mut Command) {
        command.env_clear();
        for (name, value) in &self.vars {
            command.env(name, value);
        }
    }
}

fn is_allowed(name: &str, brain: Brain) -> bool {
    HOST_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
        || (brain == Brain::Go && HOME_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

pub fn build(i: &Inputs) -> BrainEnv {
    let mut vars: Vec<(String, String)> = Vec::new();
    for (name, value) in &i.host.values {
        if is_allowed(name, i.brain) && !vars.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) {
            vars.push((name.clone(), value.clone()));
        }
    }
    if i.host.get("PATH").is_none() {
        vars.push(("PATH".into(), String::new()));
    }
    let provider = match i.brain {
        Brain::DeepSeek => wire_names::DEEPSEEK,
        Brain::Go => wire_names::GO,
    };
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut own = vec![
        ("FISHD_PROVIDER", provider.to_string()),
        ("FISHD_VAULT", path(i.vault)),
        ("FISHD_PERSONA_DIR", path(i.persona_dir)),
        ("FISHD_STATE_DIR", path(i.state_dir)),
        ("FISHD_PACK", i.pack.to_string()),
        ("FISHD_VOICE", "0".to_string()),
        ("FISHD_GATE", "heuristic".to_string()),
        ("FISHD_SHARE_DIR", path(i.share_dir)),
        ("PM_POTATO_URL", i.url.to_string()),
        ("PM_POTATO_TOKEN", i.token.to_string()),
        ("PM_POTATO_BOT", i.bot.to_string()),
    ];
    if let Some(model) = i.model.filter(|m| !m.trim().is_empty()) {
        own.push(("FISHD_MODEL", model.trim().to_string()));
    }
    vars.extend(own.into_iter().map(|(k, v)| (k.to_string(), v)));
    if let (Brain::DeepSeek, Some(key)) = (i.brain, i.deepseek_key.filter(|k| !k.is_empty())) {
        vars.push((KEY_NAME.to_string(), key.to_string()));
    }
    BrainEnv { vars }
}
