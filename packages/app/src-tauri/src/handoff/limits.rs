//! The `[limits]` table: what an agent's screen says when its plan runs out, as data.
//!
//! Every agent profile owns its table. A code profile embeds `limits.toml` with `include_str!`, a
//! TOML agent definition carries the same table inline (`[limits]`). Generic code reads only the
//! compiled form ([`CompiledLimits`]); it never names an agent. The table is closed
//! (`deny_unknown_fields`): there is no key that presses anything, so a definition cannot make
//! detection type into a pane. (The design reserves `cancel_self_resume` and `reset_gate` for the
//! hand-off step; they are not part of this schema yet, and `validate_limits` would refuse them
//! as unknown fields.)

use crate::tui::declarative::schema::DefError;
use regex::{Regex, RegexBuilder};
use serde::Deserialize;

pub const SUPPORTED_LIMITS_SCHEMA: i64 = 1;
/// A vendor of `varies` marks an agent whose billing vendor is whatever provider the user set up
/// (OpenCode, Kilo): no automatic action may ever key off it.
pub const VENDOR_VARIES: &str = "varies";

/// Can the CLI wait out its limit and carry on by itself?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfResume {
    /// It cannot.
    #[default]
    None,
    /// It can, behind a setting (a rule marked `armed` shows the setting is on).
    Setting,
    /// It always does.
    Always,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsFile {
    pub limits: LimitsTable,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsTable {
    pub schema: i64,
    /// The BILLING vendor ("anthropic", "openai"), or `varies`. A lowercase slug.
    pub vendor: String,
    /// CLI versions these rules were checked against; absent = the profile's `tested_versions`.
    pub tested: Option<String>,
    #[serde(default)]
    pub self_resume: SelfResume,
    /// Only the last N non-blank rows of the visible screen are searched.
    #[serde(default = "default_tail_lines")]
    pub tail_lines: usize,
    /// How long a match must stay on screen before it counts as `Stable`.
    #[serde(default = "default_stable_ms")]
    pub stable_ms: u64,
    /// Row markers that open a transcript block: the agent's own reply or tool row, the person's own
    /// prompt echo. Inside such a block only a row that starts with `notice_glyph` can be a notice,
    /// so an agent quoting the limit sentence, or a person pasting it, does not look like a limit.
    #[serde(default)]
    pub block_markers: Vec<String>,
    /// The row prefix of a notice the CLI prints under a block (`⎿`).
    pub notice_glyph: Option<String>,
    /// The plan ran out. Several rules may match at once; the first listed one leads, an `armed`
    /// one wins.
    #[serde(default)]
    pub usage: Vec<UsageRule>,
    /// The plan does not include the agent.
    #[serde(default)]
    pub unavailable: Vec<LimitRule>,
    /// The vendor's servers throttle; the CLI retries by itself.
    #[serde(default)]
    pub transient: Vec<LimitRule>,
    /// The CLI gave up retrying and the turn ended.
    #[serde(default)]
    pub transient_final: Vec<LimitRule>,
    /// "You have less than 25% left".
    #[serde(default)]
    pub warning: Vec<LimitRule>,
    /// A menu with the vendor's own (possibly paid) choices. Puppet Master never answers it.
    #[serde(default)]
    pub menu: Vec<LimitRule>,
    pub reset_formats: Option<ResetFormats>,
}

fn default_tail_lines() -> usize {
    12
}

fn default_stable_ms() -> u64 {
    2000
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageRule {
    pub id: String,
    pub regex: String,
    /// The CLI shows it will continue on its own (its "continue automatically" wait is armed).
    #[serde(default)]
    pub armed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitRule {
    pub id: Option<String>,
    pub regex: String,
}

/// How the text after "resets"/"try again at" is read. Each entry is a regex with the named groups
/// `hour` (1-12), `ampm`, and optionally `minute`, `mon`, `day`, `year`; they are tried in order.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetFormats {
    pub formats: Vec<String>,
}

/// A rule with its regex built. Case-sensitive on purpose: the text is the CLI's own.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub id: String,
    pub armed: bool,
    pub regex: Regex,
}

#[derive(Debug, Clone)]
pub struct CompiledLimits {
    pub table: LimitsTable,
    pub usage: Vec<CompiledRule>,
    pub unavailable: Vec<CompiledRule>,
    pub transient: Vec<CompiledRule>,
    pub transient_final: Vec<CompiledRule>,
    pub warning: Vec<CompiledRule>,
    pub menu: Vec<CompiledRule>,
    pub reset_formats: Vec<Regex>,
}

fn build(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern).multi_line(true).size_limit(1 << 20).build()
}

fn build_rules(
    rules: impl Iterator<Item = (String, bool, String)>,
    path: &str,
) -> Result<Vec<CompiledRule>, DefError> {
    let mut out = Vec::new();
    for (i, (id, armed, pattern)) in rules.enumerate() {
        let regex = build(&pattern)
            .map_err(|e| DefError::new("AGENT_DEF_LIMITS_REGEX", format!("{path}.{i}.regex"), e.to_string()))?;
        out.push(CompiledRule { id, armed, regex });
    }
    Ok(out)
}

impl CompiledLimits {
    /// Compile a validated table.
    pub fn compile(table: LimitsTable) -> Result<Self, DefError> {
        let plain = |rules: &[LimitRule], name: &str| {
            build_rules(
                rules
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (r.id.clone().unwrap_or_else(|| format!("{name}_{i}")), false, r.regex.clone())),
                &format!("limits.{name}"),
            )
        };
        let usage = build_rules(
            table.usage.iter().map(|r| (r.id.clone(), r.armed, r.regex.clone())),
            "limits.usage",
        )?;
        let unavailable = plain(&table.unavailable, "unavailable")?;
        let transient = plain(&table.transient, "transient")?;
        let transient_final = plain(&table.transient_final, "transient_final")?;
        let warning = plain(&table.warning, "warning")?;
        let menu = plain(&table.menu, "menu")?;
        let mut reset_formats = Vec::new();
        for (i, pattern) in table.reset_formats.iter().flat_map(|f| f.formats.iter()).enumerate() {
            let re = RegexBuilder::new(pattern).case_insensitive(true).size_limit(1 << 20).build().map_err(|e| {
                DefError::new("AGENT_DEF_LIMITS_REGEX", format!("limits.reset_formats.formats.{i}"), e.to_string())
            })?;
            reset_formats.push(re);
        }
        Ok(Self { table, usage, unavailable, transient, transient_final, warning, menu, reset_formats })
    }

    /// Parse embedded TOML text (a code profile's `limits.toml`) and compile it. A broken embedded
    /// table is a bug in this repository, caught by the tests; at run time it yields `None`.
    pub fn from_toml(text: &str) -> Option<Self> {
        let file: LimitsFile = toml::from_str(text).ok()?;
        if !validate_limits(&file.limits).is_empty() {
            return None;
        }
        Self::compile(file.limits).ok()
    }

    pub fn vendor(&self) -> &str {
        &self.table.vendor
    }

    /// The agent's billing vendor is not known (whatever provider the user set up).
    pub fn vendor_varies(&self) -> bool {
        self.table.vendor == VENDOR_VARIES
    }
}

/// Problems with a `[limits]` table; empty means usable. Codes: `AGENT_DEF_LIMITS_NO_VENDOR`,
/// `AGENT_DEF_LIMITS_REGEX` (does not compile, or a `usage` rule that matches the empty string),
/// `AGENT_DEF_INCONSISTENT` (schema, tail, stable window, markers, formats).
pub fn validate_limits(table: &LimitsTable) -> Vec<DefError> {
    let mut errors: Vec<DefError> = Vec::new();
    fn add(errors: &mut Vec<DefError>, code: &'static str, path: &str, message: &str) {
        errors.push(DefError::new(code, path, message));
    }
    if table.schema != SUPPORTED_LIMITS_SCHEMA {
        add(&mut errors, "AGENT_DEF_INCONSISTENT", "limits.schema", "limits schema must be 1");
    }
    let slug = !table.vendor.is_empty()
        && table.vendor.len() <= 32
        && table.vendor.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if !slug {
        add(
            &mut errors,
            "AGENT_DEF_LIMITS_NO_VENDOR",
            "limits.vendor",
            "vendor is required: the billing vendor as a lowercase slug (or `varies`)",
        );
    }
    if !(1..=40).contains(&table.tail_lines) {
        add(&mut errors, "AGENT_DEF_INCONSISTENT", "limits.tail_lines", "tail_lines must be 1-40");
    }
    if !(200..=30_000).contains(&table.stable_ms) {
        add(&mut errors, "AGENT_DEF_INCONSISTENT", "limits.stable_ms", "stable_ms must be 200-30000");
    }
    if let Some(range) = &table.tested {
        if semver::VersionReq::parse(range).is_err() {
            add(&mut errors, "AGENT_DEF_INCONSISTENT", "limits.tested", "tested must be a version range such as \">=2.1.300, <2.2\"");
        }
    }
    if table.block_markers.len() > 8
        || table.block_markers.iter().any(|m| m.is_empty() || m.chars().count() > 4 || m.chars().any(char::is_whitespace))
    {
        add(&mut errors, "AGENT_DEF_INCONSISTENT", "limits.block_markers", "at most 8 markers of 1-4 visible characters");
    }
    if let Some(glyph) = &table.notice_glyph {
        if glyph.is_empty() || glyph.chars().count() > 4 {
            add(&mut errors, "AGENT_DEF_INCONSISTENT", "limits.notice_glyph", "notice_glyph is 1-4 characters");
        }
    }
    let total = table.usage.len()
        + table.unavailable.len()
        + table.transient.len()
        + table.transient_final.len()
        + table.warning.len()
        + table.menu.len();
    if total > 64 {
        add(&mut errors, "AGENT_DEF_TOO_LARGE", "limits", "at most 64 rules");
    }
    fn check(errors: &mut Vec<DefError>, path: String, pattern: &str, must_not_be_empty: bool) {
        match build(pattern) {
            Err(e) => errors.push(DefError::new("AGENT_DEF_LIMITS_REGEX", path, e.to_string())),
            Ok(re) if must_not_be_empty && re.is_match("") => {
                errors.push(DefError::new("AGENT_DEF_LIMITS_REGEX", path, "a rule must not match the empty string"))
            }
            Ok(_) => {}
        }
    }
    for (i, rule) in table.usage.iter().enumerate() {
        check(&mut errors, format!("limits.usage.{i}.regex"), &rule.regex, true);
    }
    for (name, rules) in [
        ("unavailable", &table.unavailable),
        ("transient", &table.transient),
        ("transient_final", &table.transient_final),
        ("warning", &table.warning),
        ("menu", &table.menu),
    ] {
        for (i, rule) in rules.iter().enumerate() {
            check(&mut errors, format!("limits.{name}.{i}.regex"), &rule.regex, true);
        }
    }
    if let Some(formats) = &table.reset_formats {
        if formats.formats.is_empty() || formats.formats.len() > 8 {
            add(&mut errors, "AGENT_DEF_INCONSISTENT", "limits.reset_formats.formats", "1-8 reset formats");
        }
        for (i, pattern) in formats.formats.iter().enumerate() {
            match RegexBuilder::new(pattern).case_insensitive(true).size_limit(1 << 20).build() {
                Err(e) => errors.push(DefError::new("AGENT_DEF_LIMITS_REGEX", format!("limits.reset_formats.formats.{i}"), e.to_string())),
                Ok(re) => {
                    let names: Vec<&str> = re.capture_names().flatten().collect();
                    if !names.contains(&"hour") || !names.contains(&"ampm") {
                        errors.push(DefError::new(
                            "AGENT_DEF_INCONSISTENT",
                            format!("limits.reset_formats.formats.{i}"),
                            "a reset format needs the groups `hour` and `ampm`",
                        ));
                    }
                }
            }
        }
    }
    errors
}
