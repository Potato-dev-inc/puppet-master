//! SPEC 7.1 as serde structs. Every table uses `deny_unknown_fields` (FMT-2).
//!
//! Fields whose value set is a closed list of strings that has its own error code (`when`,
//! `kind`, ids) stay strings here and are checked by `validate`; closed enums with no dedicated
//! code (`format`, `pick`, `cwd`, ...) are real enums, so a bad value is a path-carrying parse error.

use std::collections::BTreeMap;

use serde::Deserialize;

pub const SUPPORTED_SCHEMA_VERSIONS: &[i64] = &[1];

/// A load or validation failure: SPEC 7.4 code, dotted field path and a human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefError {
    pub code: &'static str,
    pub path: String,
    pub message: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

impl DefError {
    pub fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code, path: path.into(), message: message.into(), line: None, col: None }
    }
}

impl std::fmt::Display for DefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at `{}`: {}", self.code, self.path, self.message)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema_version: i64,
    pub agent: AgentTable,
    pub binary: BinaryTable,
    #[serde(default)]
    pub features: FeaturesTable,
    #[serde(default)]
    pub run_options: RunOptionsTable,
    #[serde(default)]
    pub env: EnvTable,
    pub headless: Option<HeadlessTable>,
    pub tui: Option<TuiTable>,
    #[serde(default)]
    pub standby: StandbyTable,
    #[serde(default)]
    pub model: ModelTable,
    #[serde(default)]
    pub context: ContextTable,
    pub mcp_host: Option<McpHostTable>,
    /// Extra risk patterns for approval cards (can only raise a result).
    pub risk: Option<RiskTable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Cli,
    Generic,
    /// Reserved: rejected by `validate` with `AGENT_DEF_FIELD_RESERVED`.
    NativeProtocol,
    /// Reserved: rejected by `validate` with `AGENT_DEF_FIELD_RESERVED`.
    Shell,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTable {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_kind")]
    pub kind: Kind,
    #[serde(default)]
    pub experimental: bool,
    #[serde(default)]
    pub allow_interpreter: bool,
    /// Reserved (SPEC 7.1): any value is rejected in v1.
    pub base: Option<String>,
}

fn default_kind() -> Kind {
    Kind::Cli
}

impl AgentTable {
    /// Explicit icon, else the first two letters of the id upper-cased.
    pub fn icon_or_default(&self) -> String {
        match &self.icon {
            Some(icon) => icon.clone(),
            None => self.id.chars().take(2).collect::<String>().to_uppercase(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryTable {
    pub names: Vec<String>,
    #[serde(default = "default_windows_extensions")]
    pub windows_extensions: Vec<String>,
    #[serde(default)]
    pub shim_native_globs: Vec<String>,
    #[serde(default)]
    pub allow_cmd_shim: bool,
    #[serde(default = "default_version_args")]
    pub version_args: Vec<String>,
    #[serde(default)]
    pub install_hint: String,
    /// How the user signs in once installed; plain text shown by the agent doctor.
    #[serde(default)]
    pub login_hint: String,
}

fn default_windows_extensions() -> Vec<String> {
    ["exe", "cmd", "bat"].iter().map(|s| s.to_string()).collect()
}

fn default_version_args() -> Vec<String> {
    vec!["--version".to_string()]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesTable {
    #[serde(default = "default_true")]
    pub dispatchable: bool,
    #[serde(default)]
    pub not_dispatchable_reason: String,
    #[serde(default)]
    pub read_only_headless: bool,
    #[serde(default)]
    pub read_only_spawned_pane: bool,
    /// The TUI reprints its whole conversation on every resize without clearing scrollback, so
    /// viewers wipe their history at the resize point. Default false.
    #[serde(default)]
    pub reprints_on_resize: bool,
    /// Derived (HDL-9) when absent; an explicit value must agree with the session selector.
    pub resumable_headless: Option<bool>,
    #[serde(default)]
    pub read_only_note: String,
}

impl Default for FeaturesTable {
    fn default() -> Self {
        Self {
            dispatchable: true,
            not_dispatchable_reason: String::new(),
            read_only_headless: false,
            read_only_spawned_pane: false,
            reprints_on_resize: false,
            resumable_headless: None,
            read_only_note: String::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunOptionsTable {
    #[serde(default)]
    pub trust_workspace: bool,
    #[serde(default)]
    pub allow_shell: bool,
    #[serde(default)]
    pub shell_default_on: bool,
    #[serde(default)]
    pub allowed_commands: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvTable {
    #[serde(default)]
    pub set: BTreeMap<String, String>,
    #[serde(default)]
    pub unset: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cwd {
    Project,
    Inherit,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalSegment {
    pub when: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdinTable {
    pub template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadlessTable {
    pub argv: Vec<String>,
    pub task_flag: Option<String>,
    pub task_separator: Option<String>,
    #[serde(default = "default_cwd")]
    pub cwd: Cwd,
    #[serde(default = "default_max_output_bytes")]
    pub max_output_bytes: u64,
    pub timeout_hint_ms: Option<u64>,
    #[serde(default)]
    pub optional: Vec<OptionalSegment>,
    pub stdin: Option<StdinTable>,
    pub output: Option<OutputTable>,
}

fn default_cwd() -> Cwd {
    Cwd::Project
}

fn default_max_output_bytes() -> u64 {
    8_388_608
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    PlainText,
    JsonSingle,
    JsonLines,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputTable {
    pub format: OutputFormat,
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
    pub result: Option<Selector>,
    pub session: Option<Selector>,
    pub model: Option<Selector>,
    pub done: Option<Selector>,
    pub error: Option<Selector>,
    /// Selector plus `warning_code`; feeds `rejected_call_warning`.
    pub rejected: Option<Selector>,
}

/// A `where` value: string, integer or bool (SPEC 7.1 selector table).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Bool(bool),
    Int(i64),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pick {
    First,
    Last,
    Join,
    MaxBy,
}

/// The selector table used by every `[headless.output.*]` entry. `warning_code` is only
/// meaningful (and only accepted by `validate`) under `rejected`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    #[serde(default, rename = "where")]
    pub where_eq: BTreeMap<String, Scalar>,
    #[serde(default)]
    pub where_regex: BTreeMap<String, String>,
    pub path: Option<String>,
    #[serde(default = "default_pick")]
    pub pick: Pick,
    #[serde(default = "default_join_with")]
    pub join_with: String,
    pub max_by: Option<String>,
    #[serde(default = "default_true")]
    pub trim: bool,
    pub regex: Option<String>,
    pub warning_code: Option<String>,
}

fn default_pick() -> Pick {
    Pick::Last
}

fn default_join_with() -> String {
    "\n".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuiTable {
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub optional: Vec<OptionalSegment>,
    pub read_only_argv: Option<Vec<String>>,
    pub idle_after_ms: Option<u64>,
    pub quiet_idle_after_ms: Option<u64>,
    pub clear_input_keys: Option<String>,
    pub clear_input_sequence: Option<String>,
    pub ready: Option<ReadyTable>,
    pub busy: Option<BusyTable>,
    pub chrome: Option<LineRegexTable>,
    pub tool_activity: Option<LineRegexTable>,
    /// Activity line patterns: matched against visible lines, newest first. The phrase is the
    /// named group `text`, else group 1, else the whole match.
    #[serde(default)]
    pub activity_regex: Vec<String>,
    pub menu: Option<MenuTable>,
    pub dialogs: Option<DialogsTable>,
    pub tidy: Option<TidyTable>,
    pub footer_model: Option<FooterModelTable>,
    #[serde(default)]
    pub prompt: Vec<PromptTable>,
    pub delivery: Option<DeliveryTable>,
    pub layout: Option<LayoutTable>,
    pub classify: Option<ClassifyTable>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadyTable {
    pub regex: Option<String>,
    #[serde(default)]
    pub not_regex: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusyTable {
    #[serde(default)]
    pub working_regex: Vec<String>,
    #[serde(default)]
    pub idle_regex: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineRegexTable {
    #[serde(default)]
    pub line_regex: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuTable {
    #[serde(default)]
    pub regex: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StripBlock {
    pub start_regex: String,
    pub end_regex: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogsTable {
    #[serde(default)]
    pub strip_blocks: Vec<StripBlock>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TidyTable {
    #[serde(default)]
    pub dedupe_adjacent_lines: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FooterModelTable {
    pub regex: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub key: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptTable {
    pub kind: String,
    pub detect_regex: String,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub broad: bool,
    pub not_when_regex: Option<String>,
}

/// `[tui.layout]`: a fixed-width right sidebar that appears when the pane is wider than
/// `sidebar_min_cols`; the rightmost `sidebar_width` columns are cropped from screen text.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutTable {
    pub sidebar_min_cols: Option<u16>,
    pub sidebar_width: Option<u16>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryTable {
    pub input_echo_regex: Option<String>,
    pub transcript_echo_regex: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifyTable {
    #[serde(default)]
    pub waiting_regex: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandbyTable {
    /// Unset means the default for the definition: opt-in with `[tui.ready]`, opt-out without it
    /// (the experimental generic path). See `DeclarativeProfile::standby_support`.
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub headless: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    #[default]
    None,
    FooterRegex,
    OutputPath,
    ProbeCommand,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelTable {
    #[serde(default)]
    pub source: ModelSource,
    pub probe_argv: Option<Vec<String>>,
    pub probe_regex: Option<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextTable {
    #[serde(default = "default_smartness")]
    pub smartness: i64,
    #[serde(default = "default_strengths")]
    pub strengths: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub best_for: Vec<String>,
}

impl Default for ContextTable {
    fn default() -> Self {
        Self {
            smartness: default_smartness(),
            strengths: default_strengths(),
            notes: Vec::new(),
            best_for: Vec::new(),
        }
    }
}

fn default_smartness() -> i64 {
    5
}

fn default_strengths() -> Vec<String> {
    vec!["implementation".to_string()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpHostFormat {
    Json,
    Toml,
    Yaml,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpHostTable {
    pub config_path: String,
    pub format: McpHostFormat,
    pub key_path: String,
    pub entry: toml::Table,
}

/// Parse a definition file. Error codes: `AGENT_DEF_INVALID_TOML` (syntax or wrong type, with the
/// field path when known), `AGENT_DEF_UNKNOWN_FIELD` (path of the unknown key),
/// `AGENT_DEF_SCHEMA_UNSUPPORTED` (missing or unsupported `schema_version`).
pub fn parse_definition(text: &str) -> Result<Definition, DefError> {
    let table: toml::Table = toml::from_str(text).map_err(|e| toml_error(text, &e, None))?;
    match table.get("schema_version") {
        Some(toml::Value::Integer(v)) if SUPPORTED_SCHEMA_VERSIONS.contains(v) => {}
        Some(toml::Value::Integer(v)) => {
            return Err(unsupported(format!("schema_version {v} is not supported")))
        }
        Some(_) => return Err(unsupported("schema_version must be an integer".to_string())),
        None => return Err(unsupported("schema_version is required".to_string())),
    }
    let de = toml::Deserializer::new(text);
    serde_path_to_error::deserialize::<_, Definition>(de).map_err(|e| {
        let path = e.path().to_string();
        toml_error(text, e.inner(), Some(path))
    })
}

fn unsupported(detail: String) -> DefError {
    let versions: Vec<String> = SUPPORTED_SCHEMA_VERSIONS.iter().map(|v| v.to_string()).collect();
    DefError::new(
        "AGENT_DEF_SCHEMA_UNSUPPORTED",
        "schema_version",
        format!("{detail}; supported versions: {}", versions.join(", ")),
    )
}

fn toml_error(text: &str, err: &toml::de::Error, path: Option<String>) -> DefError {
    let message = err.message().to_string();
    let (line, col) = match err.span() {
        Some(span) => {
            let before = &text[..span.start.min(text.len())];
            let line = before.matches('\n').count() + 1;
            let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            (Some(line), Some(col))
        }
        None => (None, None),
    };
    // serde_path_to_error prints indices as `a[0].b`; definitions use dotted paths (`a.0.b`).
    let base_path = path.filter(|p| p != ".").unwrap_or_default().replace('[', ".").replace(']', "");
    let (code, path) = match unknown_field_name(&message) {
        Some(field) => {
            // the path normally already ends with the offending key
            let full = if base_path == field || base_path.ends_with(&format!(".{field}")) {
                base_path
            } else if base_path.is_empty() {
                field
            } else {
                format!("{base_path}.{field}")
            };
            ("AGENT_DEF_UNKNOWN_FIELD", full)
        }
        None => ("AGENT_DEF_INVALID_TOML", base_path),
    };
    DefError { code, path, message, line, col }
}

fn unknown_field_name(message: &str) -> Option<String> {
    let rest = message.strip_prefix("unknown field `")?;
    Some(rest.split('`').next()?.to_string())
}

/// `[risk]`: patterns that add to the plain-language risk of an approval prompt.
///
/// ```toml
/// [[risk.patterns]]
/// match = 'deploy'
/// level = "high"
/// headline = "Deploy to production"
/// reason = "It sends this project to the live server."
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskTable {
    #[serde(default)]
    pub patterns: Vec<RiskPattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskPattern {
    /// Regex matched case-insensitively against the command text.
    #[serde(rename = "match")]
    pub pattern: String,
    pub level: crate::prompt_risk::RiskLevel,
    /// At most 60 characters, plain words.
    pub headline: String,
    /// At most 200 characters.
    #[serde(default)]
    pub reason: String,
}
