//! Puppet Master's own MCP server, handed to an agent for ONE launch (`TuiProfile::pane_mcp`).
//!
//! An agent that runs in a Puppet Master pane should have `run_agent` / `wait_agents` ready with no
//! setup. Each agent has its own way to take an MCP server for one launch without touching the
//! person's config files (a flag, an env var holding config text). That knowledge lives in the
//! agent's folder (`tui/<agent>/`) or in a TOML definition's `[pane_mcp]` table; the generic
//! orchestration is `crate::pane_mcp`.
//!
//! This file holds the shared types and the table renderer. Nothing here names an agent.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The MCP server as this app wants every pane's agent to start it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMcpServer {
    /// The name the agent shows its tools under (`puppet-master`).
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    /// Always the bridge port file of THIS app and the id of the pane the agent runs in (so the
    /// helper it starts nests under that pane).
    pub env: Vec<(String, String)>,
}

/// What a profile needs to build one launch's MCP arguments.
pub struct PaneMcpRequest<'a> {
    pub pane_id: &'a str,
    /// The arguments the person asked for; a profile leaves its own choices alone when they are there.
    pub user_args: &'a [String],
    pub server: &'a PaneMcpServer,
    /// The folder the app keeps per-launch files in. A file a profile needs is returned in
    /// `PaneMcpLaunch::files` with a path inside it; the caller writes it.
    pub dir: &'a Path,
    /// One or two sentences telling the agent the helpers exist.
    pub note: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMcpFile {
    pub path: PathBuf,
    pub content: String,
}

/// Env, arguments and files one launch gets. `Default` = nothing (the agent starts exactly as before).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneMcpLaunch {
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
    pub files: Vec<PaneMcpFile>,
}

impl PaneMcpLaunch {
    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.args.is_empty()
    }
}

/// A file name that is safe for any pane id.
pub fn safe_file_stem(pane_id: &str) -> String {
    let stem: String = pane_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(80)
        .collect();
    if stem.is_empty() { "pane".into() } else { stem }
}

/// The note's file name (one file for every pane: its text never changes).
pub const NOTE_FILE: &str = "helpers-note.md";

/// How the agent's config text is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigShape {
    /// `{"mcpServers": {name: {command, args, env}}}` (the shape most agents' MCP files use).
    McpServers,
    /// `{"mcp": {name: {type: "local", command: [..], enabled, environment}}}`.
    Opencode,
}

/// `[pane_mcp]` in an agent definition: how to hand the server to the agent for one launch.
///
/// Placeholders (in `args`, `env`, `note_args`, `note_env`): `{config}` the config text (one line of
/// JSON), `{config_file}` a file holding it, `{note}` the note text, `{note_file}` a file holding it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaneMcpTable {
    pub shape: Option<ConfigShape>,
    /// Arguments added to the launch (after the person's own).
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables added to the launch.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Added only when the note is wanted (see `note_unless_arg`).
    #[serde(default)]
    pub note_args: Vec<String>,
    #[serde(default)]
    pub note_env: BTreeMap<String, String>,
    /// The config text itself carries the note as a list of instruction files (the `opencode` shape).
    #[serde(default)]
    pub note_in_config: bool,
    /// The person already chose their own system text with one of these arguments: no note.
    #[serde(default)]
    pub note_unless_arg: Vec<String>,
    /// The launch is left alone when one of these variables is already set in the app's own
    /// environment (the person's own value for it must not be replaced).
    #[serde(default)]
    pub skip_if_env: Vec<String>,
    /// The launch is left alone when the person's arguments hold one of these.
    #[serde(default)]
    pub skip_if_arg: Vec<String>,
}

const PLACEHOLDERS: [&str; 4] = ["{config}", "{config_file}", "{note}", "{note_file}"];
const MAX_ENTRIES: usize = 16;

fn placeholders_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start..];
        match after.find('}') {
            Some(end) => {
                found.push(after[..=end].to_string());
                rest = &after[end + 1..];
            }
            None => {
                found.push(after.to_string());
                break;
            }
        }
    }
    found
}

fn all_texts(table: &PaneMcpTable) -> impl Iterator<Item = &String> {
    table
        .args
        .iter()
        .chain(table.env.values())
        .chain(table.note_args.iter())
        .chain(table.note_env.values())
}

/// Load-time check; `(path, message)` pairs. Never looked at again later.
pub fn validate_pane_mcp(table: &PaneMcpTable) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    let mut add = |path: &str, message: String| errors.push((path.to_string(), message));
    if table.shape.is_none() {
        add("pane_mcp.shape", "must be \"mcp_servers\" or \"opencode\"".into());
    }
    if table.args.len() + table.note_args.len() > MAX_ENTRIES || table.env.len() + table.note_env.len() > MAX_ENTRIES {
        add("pane_mcp", format!("at most {MAX_ENTRIES} arguments and {MAX_ENTRIES} variables"));
    }
    for text in all_texts(table) {
        if text.len() > 512 {
            add("pane_mcp", "an argument or value is longer than 512 characters".into());
        }
        for found in placeholders_in(text) {
            if !PLACEHOLDERS.contains(&found.as_str()) {
                add("pane_mcp", format!("unknown placeholder {found}; use {}", PLACEHOLDERS.join(", ")));
            }
        }
    }
    for name in table.env.keys().chain(table.note_env.keys()).chain(table.skip_if_env.iter()) {
        let ok = !name.is_empty()
            && name.len() <= 64
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit());
        if !ok {
            add("pane_mcp.env", format!("{name:?} is not a variable name"));
        }
    }
    let hands_over = table.args.iter().chain(table.env.values()).any(|t| t.contains("{config}") || t.contains("{config_file}"));
    if !hands_over {
        add("pane_mcp", "args or env must use {config} or {config_file}, or the server never reaches the agent".into());
    }
    if table.note_in_config && table.shape != Some(ConfigShape::Opencode) {
        add("pane_mcp.note_in_config", "only the \"opencode\" shape can carry the note in its config".into());
    }
    errors
}

pub fn parse_pane_mcp_toml(text: &str) -> Option<PaneMcpTable> {
    toml::from_str::<PaneMcpTable>(text).ok().filter(|t| validate_pane_mcp(t).is_empty())
}

/// The config text for `shape`; the note file is listed under `instructions` when asked.
pub fn config_value(shape: ConfigShape, server: &PaneMcpServer, instructions: Option<&str>) -> Value {
    let env: serde_json::Map<String, Value> = server.env.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    match shape {
        ConfigShape::McpServers => json!({
            "mcpServers": { server.name.clone(): { "command": server.command, "args": server.args, "env": env } }
        }),
        ConfigShape::Opencode => {
            let mut command = vec![server.command.clone()];
            command.extend(server.args.iter().cloned());
            let mut config = json!({
                "mcp": { server.name.clone(): { "type": "local", "command": command, "enabled": true, "environment": env } }
            });
            if let Some(file) = instructions {
                config["instructions"] = json!([file]);
            }
            config
        }
    }
}

fn host_path(path: &Path) -> String {
    crate::app_paths::path_for_host_config(path)
}

/// Render a definition's table for one launch. `env_set` says whether a variable is already set in
/// the app's own environment.
pub fn render(table: &PaneMcpTable, request: &PaneMcpRequest<'_>, env_set: &dyn Fn(&str) -> bool) -> PaneMcpLaunch {
    let Some(shape) = table.shape else { return PaneMcpLaunch::default() };
    if table.skip_if_env.iter().any(|name| env_set(name)) {
        return PaneMcpLaunch::default();
    }
    if table.skip_if_arg.iter().any(|flag| request.user_args.iter().any(|a| a == flag || a.starts_with(&format!("{flag}=")))) {
        return PaneMcpLaunch::default();
    }
    let with_note = !table.note_unless_arg.iter().any(|flag| request.user_args.iter().any(|a| a == flag || a.starts_with(&format!("{flag}="))));
    let note_path = request.dir.join(NOTE_FILE);
    let note_file = host_path(&note_path);
    let instructions = (table.note_in_config && with_note).then_some(note_file.as_str());
    let config = config_value(shape, request.server, instructions);
    let config_text = serde_json::to_string(&config).unwrap_or_default();
    let config_path = request.dir.join(format!("{}.mcp.json", safe_file_stem(request.pane_id)));
    let config_file = host_path(&config_path);

    let fill = |text: &str| {
        text.replace("{config_file}", &config_file)
            .replace("{config}", &config_text)
            .replace("{note_file}", &note_file)
            .replace("{note}", request.note)
    };
    let mut launch = PaneMcpLaunch::default();
    launch.args.extend(table.args.iter().map(|a| fill(a)));
    launch.env.extend(table.env.iter().map(|(k, v)| (k.clone(), fill(v))));
    if with_note {
        launch.args.extend(table.note_args.iter().map(|a| fill(a)));
        launch.env.extend(table.note_env.iter().map(|(k, v)| (k.clone(), fill(v))));
    }
    let uses = |placeholder: &str| {
        let texts = table.args.iter().chain(table.env.values()).chain(
            table.note_args.iter().chain(table.note_env.values()).filter(|_| with_note),
        );
        texts.into_iter().any(|t| t.contains(placeholder))
    };
    if uses("{config_file}") {
        launch.files.push(PaneMcpFile { path: config_path, content: config_text });
    }
    if uses("{note_file}") || instructions.is_some() {
        launch.files.push(PaneMcpFile { path: note_path, content: format!("{}\n", request.note) });
    }
    launch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> PaneMcpServer {
        PaneMcpServer {
            name: "puppet-master".into(),
            command: "C:/app/bin/puppet-master-mcp.exe".into(),
            args: vec![],
            env: vec![
                ("PUPPET_MASTER_BRIDGE_PORT_FILE".into(), "C:/data/puppet-master.bridge.port".into()),
                ("PUPPET_MASTER_PANE_ID".into(), "pane-7".into()),
            ],
        }
    }

    fn request<'a>(server: &'a PaneMcpServer, user_args: &'a [String], dir: &'a Path) -> PaneMcpRequest<'a> {
        PaneMcpRequest { pane_id: "pane-7", user_args, server, dir, note: "Use run_agent." }
    }

    fn table(text: &str) -> PaneMcpTable {
        let table: PaneMcpTable = toml::from_str(text).expect("parses");
        assert_eq!(validate_pane_mcp(&table), vec![], "valid");
        table
    }

    #[test]
    fn the_file_shape_names_the_server_command_and_this_panes_env() {
        let config = config_value(ConfigShape::McpServers, &server(), None);
        let entry = &config["mcpServers"]["puppet-master"];
        assert_eq!(entry["command"], "C:/app/bin/puppet-master-mcp.exe");
        assert_eq!(entry["env"]["PUPPET_MASTER_PANE_ID"], "pane-7");
        assert_eq!(entry["env"]["PUPPET_MASTER_BRIDGE_PORT_FILE"], "C:/data/puppet-master.bridge.port");
    }

    #[test]
    fn the_inline_shape_joins_command_and_args_and_may_list_the_note_file() {
        let mut s = server();
        s.args = vec!["--x".into()];
        let config = config_value(ConfigShape::Opencode, &s, Some("C:/data/pane-mcp/helpers-note.md"));
        assert_eq!(config["mcp"]["puppet-master"]["command"], json!(["C:/app/bin/puppet-master-mcp.exe", "--x"]));
        assert_eq!(config["mcp"]["puppet-master"]["type"], "local");
        assert_eq!(config["mcp"]["puppet-master"]["environment"]["PUPPET_MASTER_PANE_ID"], "pane-7");
        assert_eq!(config["instructions"], json!(["C:/data/pane-mcp/helpers-note.md"]));
        assert!(config_value(ConfigShape::Opencode, &s, None).get("instructions").is_none());
    }

    #[test]
    fn a_file_table_writes_the_config_file_and_names_it_in_the_arguments() {
        let t = table("shape = \"mcp_servers\"\nargs = [\"--mcp-config\", \"{config_file}\"]\nnote_args = [\"--note\", \"{note}\"]\n");
        let s = server();
        let dir = Path::new("C:/data/pane-mcp");
        let launch = render(&t, &request(&s, &[], dir), &|_| false);
        assert_eq!(launch.args, vec!["--mcp-config", "C:/data/pane-mcp/pane-7.mcp.json", "--note", "Use run_agent."]);
        assert!(launch.env.is_empty());
        assert_eq!(launch.files.len(), 1);
        assert_eq!(launch.files[0].path, dir.join("pane-7.mcp.json"));
        assert!(launch.files[0].content.contains("puppet-master-mcp.exe"));
    }

    #[test]
    fn an_inline_table_puts_the_config_in_a_variable_with_the_note_file_inside() {
        let t = table("shape = \"opencode\"\nnote_in_config = true\n[env]\nSOME_CONFIG = \"{config}\"\n");
        let s = server();
        let launch = render(&t, &request(&s, &[], Path::new("C:/d")), &|_| false);
        assert!(launch.args.is_empty());
        let (name, value) = &launch.env[0];
        assert_eq!(name, "SOME_CONFIG");
        let parsed: Value = serde_json::from_str(value).expect("one JSON text");
        assert_eq!(parsed["instructions"], json!(["C:/d/helpers-note.md"]));
        assert_eq!(launch.files.len(), 1, "only the note file");
        assert_eq!(launch.files[0].path, Path::new("C:/d").join(NOTE_FILE));
    }

    #[test]
    fn a_variable_the_person_already_set_is_never_replaced() {
        let t = table("shape = \"opencode\"\nskip_if_env = [\"SOME_CONFIG\"]\n[env]\nSOME_CONFIG = \"{config}\"\n");
        let s = server();
        let launch = render(&t, &request(&s, &[], Path::new("C:/d")), &|name| name == "SOME_CONFIG");
        assert_eq!(launch, PaneMcpLaunch::default());
    }

    #[test]
    fn the_note_is_left_out_when_the_person_chose_their_own_system_text() {
        let t = table("shape = \"mcp_servers\"\nargs = [\"--mcp-config\", \"{config_file}\"]\nnote_args = [\"--append\", \"{note}\"]\nnote_unless_arg = [\"--append\"]\n");
        let s = server();
        let mine = vec!["--append=their text".to_string()];
        let launch = render(&t, &request(&s, &mine, Path::new("C:/d")), &|_| false);
        assert_eq!(launch.args, vec!["--mcp-config", "C:/d/pane-7.mcp.json"], "the server stays, the note goes");
    }

    #[test]
    fn a_pane_id_never_escapes_the_folder() {
        assert_eq!(safe_file_stem("../../etc/x y"), "______etc_x_y");
        assert_eq!(safe_file_stem(""), "pane");
        assert_eq!(safe_file_stem(&"a".repeat(200)).len(), 80);
    }

    #[test]
    fn the_load_check_names_what_is_wrong() {
        let bad = |text: &str| validate_pane_mcp(&toml::from_str::<PaneMcpTable>(text).unwrap());
        assert!(bad("args = [\"{config_file}\"]").iter().any(|(p, _)| p == "pane_mcp.shape"));
        assert!(bad("shape = \"mcp_servers\"\nargs = [\"{secret}\"]").iter().any(|(_, m)| m.contains("unknown placeholder")));
        assert!(bad("shape = \"mcp_servers\"\nargs = [\"--x\"]").iter().any(|(_, m)| m.contains("never reaches")));
        assert!(bad("shape = \"mcp_servers\"\nnote_in_config = true\nargs = [\"{config}\"]").iter().any(|(p, _)| p == "pane_mcp.note_in_config"));
        assert!(bad("shape = \"mcp_servers\"\nargs = [\"{config}\"]\n[env]\n\"BAD NAME\" = \"x\"").iter().any(|(p, _)| p == "pane_mcp.env"));
        assert!(toml::from_str::<PaneMcpTable>("shape = \"mcp_servers\"\nbogus = 1").is_err(), "unknown keys are refused");
    }
}
