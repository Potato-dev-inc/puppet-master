//! The language rules, loaded from the TOML tables in `rules/`. Nothing in the guard's code names a
//! language: a table lists a language's test globs, how to read its comments and strings, and the
//! patterns for tests, assertions, skips, focus, silencing markers, loosened assertions and config
//! red flags. Adding a language is adding a table (see `rules/go.toml` for the shortest one).

use std::path::Path;

use glob::{MatchOptions, Pattern};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

use super::model::CheckKind;

const COMMON: &str = include_str!("rules/common.toml");
const BUILTIN: [(&str, &str); 5] = [
    ("js", include_str!("rules/js.toml")),
    ("python", include_str!("rules/python.toml")),
    ("rust", include_str!("rules/rust.toml")),
    ("go", include_str!("rules/go.toml")),
    ("other", include_str!("rules/other.toml")),
];

// ---------------------------------------------------------------------------------------------
// Raw (serde) shapes: what a table file looks like.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct CommonFile {
    never_dirs: Vec<String>,
    snapshot_globs: Vec<String>,
    #[serde(default)]
    max_file_bytes: Option<u64>,
    #[serde(default)]
    content_cap_bytes: Option<u64>,
    #[serde(default)]
    max_files: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct EcosystemFile {
    id: String,
    #[serde(default = "yes")]
    scannable: bool,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    test_globs: Vec<String>,
    /// A source file whose text matches one of these is a test file (Rust unit tests live in `src`).
    #[serde(default)]
    test_markers: Vec<String>,
    #[serde(default)]
    lexer: LexerSpec,
    #[serde(default)]
    names: NamesSpec,
    #[serde(default)]
    test: Vec<PatternSpec>,
    #[serde(default)]
    assertion: Vec<PatternSpec>,
    #[serde(default)]
    skip: Vec<PatternSpec>,
    #[serde(default)]
    focus: Vec<PatternSpec>,
    #[serde(default)]
    silence: Vec<PatternSpec>,
    #[serde(default)]
    tautology: Vec<PatternSpec>,
    #[serde(default)]
    loosen: Vec<LoosenSpec>,
    #[serde(default)]
    self_compare: Vec<PatternSpec>,
    #[serde(default)]
    tolerance: Vec<ToleranceSpec>,
    #[serde(default)]
    config: Vec<ConfigSpec>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
pub struct LexerSpec {
    #[serde(default)]
    pub line_comments: Vec<String>,
    #[serde(default)]
    pub block_comments: Vec<(String, String)>,
    /// Tokens that are skipped whole, such as the Rust character literal `'"'`.
    #[serde(default)]
    pub skip_tokens: Vec<String>,
    #[serde(default)]
    pub strings: Vec<StringSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StringSpec {
    pub open: String,
    pub close: String,
    #[serde(default)]
    pub escape: String,
    #[serde(default)]
    pub multiline: bool,
}

#[derive(Debug, Default, Deserialize)]
struct NamesSpec {
    /// A regex with a `name` group, run on the raw line from the start of the match: the first quoted
    /// string after `it.skip(` is the test's name.
    #[serde(default)]
    quoted: String,
    /// A regex with a `name` group that finds a test definition line (`def test_x(`, `fn name(`).
    #[serde(default)]
    def: String,
}

#[derive(Debug, Deserialize)]
struct PatternSpec {
    regex: String,
    #[serde(default)]
    label: String,
    /// `quoted`, `next_def` or `prev_def`.
    #[serde(default)]
    name_from: String,
    /// For silencing markers: the check whose errors it hides (`typecheck`, `lint`).
    #[serde(default)]
    check: String,
}

#[derive(Debug, Deserialize)]
struct LoosenSpec {
    #[serde(default)]
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct ToleranceSpec {
    regex: String,
    /// `down`: a smaller number is looser (decimal places). `up`: a bigger number is looser (a margin).
    direction: String,
}

#[derive(Debug, Default, Deserialize)]
struct ConfigSpec {
    glob: Vec<String>,
    /// `json`, `jsonc`, `toml`, `ini` or `text`.
    #[serde(default)]
    format: String,
    /// Top-level keys (json), table prefixes (toml) or section names (ini) that are compared. Empty
    /// compares the whole file.
    #[serde(default)]
    sections: Vec<String>,
    /// The check this config belongs to. Lint and type-check config is protected only when that
    /// check is in play.
    #[serde(default)]
    check: String,
    /// For text configs: only lines inside the block that starts at a line matching this count.
    #[serde(default)]
    block: String,
    /// The section whose entries are named scripts (`scripts` in package.json); `script` rules read it.
    #[serde(default)]
    scripts_section: String,
    #[serde(default)]
    script: Vec<ScriptSpec>,
    #[serde(default)]
    flag: Vec<FlagSpec>,
    #[serde(default)]
    threshold: Vec<ThresholdSpec>,
}

#[derive(Debug, Deserialize)]
struct ScriptSpec {
    /// A regex for the script's name (package.json `scripts`).
    key: String,
    #[serde(default)]
    check: String,
}

#[derive(Debug, Deserialize)]
struct FlagSpec {
    regex: String,
    /// `checks` (how checks run), `focus` or `snapshots`.
    #[serde(default)]
    kind: String,
    /// The plain phrase after "now": "leaves out some tests".
    #[serde(default)]
    say: String,
    /// `added` (default) or `removed`: the regex runs on lines that came or went.
    #[serde(default)]
    on: String,
}

#[derive(Debug, Deserialize)]
struct ThresholdSpec {
    /// Named groups `key` and `n`.
    regex: String,
}

// ---------------------------------------------------------------------------------------------
// Compiled rules.
// ---------------------------------------------------------------------------------------------

pub struct Glob {
    full: Pattern,
    /// For `**/x`: also `x` at the root (the glob crate needs a directory before the `/`).
    rooted: Option<Pattern>,
}

const MATCH_OPTS: MatchOptions = MatchOptions { case_sensitive: true, require_literal_separator: true, require_literal_leading_dot: false };

impl Glob {
    pub fn parse(pattern: &str) -> Option<Glob> {
        let full = Pattern::new(pattern).ok()?;
        let rooted = pattern.strip_prefix("**/").and_then(|rest| Pattern::new(rest).ok());
        Some(Glob { full, rooted })
    }

    pub fn is_match(&self, path: &str) -> bool {
        self.full.matches_with(path, MATCH_OPTS) || self.rooted.as_ref().is_some_and(|p| p.matches_with(path, MATCH_OPTS))
    }
}

/// A list of globs (with `{a,b}` expanded) that matches when any one does.
pub struct GlobSet(Vec<Glob>);

impl GlobSet {
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> GlobSet {
        let mut globs = Vec::new();
        for pattern in patterns {
            for expanded in expand_braces(pattern.as_ref()) {
                if let Some(glob) = Glob::parse(&expanded) {
                    globs.push(glob);
                }
            }
        }
        GlobSet(globs)
    }

    pub fn is_match(&self, path: &str) -> bool {
        self.0.iter().any(|g| g.is_match(path))
    }

}

/// `a.{x,y}` becomes `a.x`, `a.y`. One level of nesting per brace pair; several pairs multiply.
pub fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else { return vec![pattern.to_string()] };
    let Some(close_rel) = pattern[open..].find('}') else { return vec![pattern.to_string()] };
    let close = open + close_rel;
    let (head, body, tail) = (&pattern[..open], &pattern[open + 1..close], &pattern[close + 1..]);
    let mut out = Vec::new();
    for alt in body.split(',') {
        for rest in expand_braces(tail) {
            out.push(format!("{head}{alt}{rest}"));
        }
    }
    out
}

pub struct Pat {
    pub regex: Regex,
    pub label: String,
    pub name_from: NameFrom,
    pub check: Option<CheckKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameFrom {
    None,
    Quoted,
    NextDef,
    PrevDef,
}

pub struct Loosen {
    pub from: Option<Regex>,
    pub to: Regex,
}

pub struct Tolerance {
    pub regex: Regex,
    pub looser_when_smaller: bool,
}

pub struct Script {
    pub key: Regex,
    pub check: CheckKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    Checks,
    Focus,
    Snapshots,
}

pub struct Flag {
    pub regex: Regex,
    pub kind: FlagKind,
    pub say: String,
    pub on_removed: bool,
}

pub struct ConfigRule {
    pub globs: GlobSet,
    pub format: ConfigFormat,
    pub sections: Vec<String>,
    pub check: CheckKind,
    pub block: Option<Regex>,
    /// The flattened path prefix of the scripts section, e.g. `scripts.`.
    pub scripts_prefix: Option<String>,
    pub scripts: Vec<Script>,
    pub flags: Vec<Flag>,
    pub thresholds: Vec<Regex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Json,
    Jsonc,
    Toml,
    Ini,
    Text,
}

pub struct Ecosystem {
    pub id: String,
    pub scannable: bool,
    pub extensions: Vec<String>,
    pub test_globs: GlobSet,
    pub test_markers: Vec<Regex>,
    pub lexer: LexerSpec,
    pub quoted: Option<Regex>,
    pub def: Option<Regex>,
    pub test: Vec<Pat>,
    pub assertion: Vec<Pat>,
    pub skip: Vec<Pat>,
    pub focus: Vec<Pat>,
    pub silence: Vec<Pat>,
    pub tautology: Vec<Pat>,
    pub loosen: Vec<Loosen>,
    pub self_compare: Vec<Regex>,
    pub tolerance: Vec<Tolerance>,
    pub config: Vec<ConfigRule>,
}

impl Ecosystem {
    pub fn owns_extension(&self, path: &str) -> bool {
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }
}

pub struct Rules {
    pub ecosystems: Vec<Ecosystem>,
    pub never_dirs: Vec<String>,
    pub snapshot_globs: GlobSet,
    /// A protected file bigger than this keeps only its hash.
    pub max_file_bytes: u64,
    /// The snapshot keeps file text up to this many bytes in total.
    pub content_cap_bytes: u64,
    /// The walk stops after this many files.
    pub max_files: usize,
}

fn rx(pattern: &str, context: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|e| format!("{context}: bad regex `{pattern}`: {e}"))
}

fn pats(specs: &[PatternSpec], context: &str) -> Result<Vec<Pat>, String> {
    specs
        .iter()
        .map(|s| {
            Ok(Pat {
                regex: rx(&s.regex, context)?,
                label: s.label.clone(),
                name_from: match s.name_from.as_str() {
                    "" => NameFrom::None,
                    "quoted" => NameFrom::Quoted,
                    "next_def" => NameFrom::NextDef,
                    "prev_def" => NameFrom::PrevDef,
                    other => return Err(format!("{context}: unknown name_from `{other}`")),
                },
                check: match s.check.as_str() {
                    "" => None,
                    other => Some(CheckKind::parse(other).ok_or_else(|| format!("{context}: unknown check `{other}`"))?),
                },
            })
        })
        .collect()
}

fn opt_rx(pattern: &str, context: &str) -> Result<Option<Regex>, String> {
    if pattern.is_empty() {
        Ok(None)
    } else {
        rx(pattern, context).map(Some)
    }
}

fn compile(file: EcosystemFile) -> Result<Ecosystem, String> {
    let ctx = file.id.clone();
    let mut config = Vec::new();
    for c in &file.config {
        let check = if c.check.is_empty() {
            CheckKind::Tests
        } else {
            CheckKind::parse(&c.check).ok_or_else(|| format!("{ctx}: unknown check `{}`", c.check))?
        };
        config.push(ConfigRule {
            globs: GlobSet::new(&c.glob),
            format: match c.format.as_str() {
                "json" => ConfigFormat::Json,
                "jsonc" => ConfigFormat::Jsonc,
                "toml" => ConfigFormat::Toml,
                "ini" => ConfigFormat::Ini,
                "" | "text" => ConfigFormat::Text,
                other => return Err(format!("{ctx}: unknown config format `{other}`")),
            },
            sections: c.sections.clone(),
            check,
            block: opt_rx(&c.block, &ctx)?,
            scripts_prefix: (!c.scripts_section.is_empty()).then(|| format!("{}.", c.scripts_section)),
            scripts: c
                .script
                .iter()
                .map(|s| {
                    Ok(Script {
                        key: rx(&s.key, &ctx)?,
                        check: if s.check.is_empty() {
                            CheckKind::Tests
                        } else {
                            CheckKind::parse(&s.check).ok_or_else(|| format!("{ctx}: unknown check `{}`", s.check))?
                        },
                    })
                })
                .collect::<Result<_, String>>()?,
            flags: c
                .flag
                .iter()
                .map(|f| {
                    Ok(Flag {
                        regex: rx(&f.regex, &ctx)?,
                        kind: match f.kind.as_str() {
                            "" | "checks" => FlagKind::Checks,
                            "focus" => FlagKind::Focus,
                            "snapshots" => FlagKind::Snapshots,
                            other => return Err(format!("{ctx}: unknown flag kind `{other}`")),
                        },
                        say: f.say.clone(),
                        on_removed: f.on == "removed",
                    })
                })
                .collect::<Result<_, String>>()?,
            thresholds: c.threshold.iter().map(|t| rx(&t.regex, &ctx)).collect::<Result<_, _>>()?,
        });
    }
    Ok(Ecosystem {
        scannable: file.scannable,
        extensions: file.extensions.clone(),
        test_globs: GlobSet::new(&file.test_globs),
        test_markers: file.test_markers.iter().map(|m| rx(m, &ctx)).collect::<Result<_, _>>()?,
        lexer: file.lexer,
        quoted: opt_rx(&file.names.quoted, &ctx)?,
        def: opt_rx(&file.names.def, &ctx)?,
        test: pats(&file.test, &ctx)?,
        assertion: pats(&file.assertion, &ctx)?,
        skip: pats(&file.skip, &ctx)?,
        focus: pats(&file.focus, &ctx)?,
        silence: pats(&file.silence, &ctx)?,
        tautology: pats(&file.tautology, &ctx)?,
        loosen: file
            .loosen
            .iter()
            .map(|l| Ok(Loosen { from: opt_rx(&l.from, &ctx)?, to: rx(&l.to, &ctx)? }))
            .collect::<Result<_, String>>()?,
        self_compare: file.self_compare.iter().map(|p| rx(&p.regex, &ctx)).collect::<Result<_, _>>()?,
        tolerance: file
            .tolerance
            .iter()
            .map(|t| Ok(Tolerance { regex: rx(&t.regex, &ctx)?, looser_when_smaller: t.direction == "down" }))
            .collect::<Result<_, String>>()?,
        config,
        id: file.id,
    })
}

impl Rules {
    /// Build rule sets from table sources. The built-in tables go through here too.
    pub fn from_sources(common: &str, ecosystems: &[&str]) -> Result<Rules, String> {
        let common: CommonFile = toml::from_str(common).map_err(|e| format!("common.toml: {e}"))?;
        let mut compiled = Vec::new();
        for source in ecosystems {
            let file: EcosystemFile = toml::from_str(source).map_err(|e| format!("rules table: {e}"))?;
            compiled.push(compile(file)?);
        }
        Ok(Rules {
            ecosystems: compiled,
            never_dirs: common.never_dirs,
            snapshot_globs: GlobSet::new(&common.snapshot_globs),
            max_file_bytes: common.max_file_bytes.unwrap_or(1_048_576),
            content_cap_bytes: common.content_cap_bytes.unwrap_or(20 * 1024 * 1024),
            max_files: common.max_files.unwrap_or(200_000),
        })
    }

    /// The tables that ship with the app.
    pub fn builtin() -> &'static Rules {
        static RULES: Lazy<Rules> = Lazy::new(|| {
            let sources: Vec<&str> = BUILTIN.iter().map(|(_, s)| *s).collect();
            Rules::from_sources(COMMON, &sources).expect("the built-in tamper rule tables are valid")
        });
        &RULES
    }

    pub fn ecosystem(&self, id: &str) -> Option<&Ecosystem> {
        self.ecosystems.iter().find(|e| e.id == id)
    }

    /// The ecosystem whose extension list names this file.
    pub fn ecosystem_for(&self, path: &str) -> Option<&Ecosystem> {
        self.ecosystems.iter().find(|e| e.owns_extension(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_tables_compile() {
        let rules = Rules::builtin();
        let ids: Vec<&str> = rules.ecosystems.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["js", "python", "rust", "go", "other"]);
        assert!(!rules.never_dirs.is_empty());
    }

    #[test]
    fn braces_expand_and_globs_match_at_the_root() {
        assert_eq!(expand_braces("a.{x,y}"), ["a.x", "a.y"]);
        assert_eq!(expand_braces("{a,b}/{c,d}").len(), 4);
        let set = GlobSet::new(&["**/*.{test,spec}.ts", "**/tests/**/*.py"]);
        assert!(set.is_match("cart.test.ts"));
        assert!(set.is_match("src/deep/cart.spec.ts"));
        assert!(set.is_match("tests/test_cart.py"));
        assert!(set.is_match("pkg/tests/unit/test_cart.py"));
        assert!(!set.is_match("src/cart.ts"));
    }

    #[test]
    fn a_bad_table_is_reported_not_swallowed() {
        let bad = "id = \"x\"\n[[skip]]\nregex = \"(\"\n";
        let err = Rules::from_sources(COMMON, &[bad]).err().expect("must fail");
        assert!(err.contains("bad regex"), "{err}");
    }
}
