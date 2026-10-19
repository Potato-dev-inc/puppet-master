//! The redaction engine: which text is secret or personal, found as byte spans of the input.
//!
//! Promoted from `agent_definitions_recorder/redact.rs` (which now re-exports this file's public
//! items). Finding *spans* instead of rewriting the text lets one pass cover text that arrives in
//! pieces: a secret split across two PTY chunks is found in the joined text and each chunk is masked
//! for the part it holds (`mask_pieces`, used by the flight cast).
//!
//! Detectors, in precision order:
//! 1. exact values this process knows are secret (env vars, the OS credential store, toolbelt and
//!    OpenCode key profiles, anything registered at runtime), case-insensitive;
//! 2. vendor token shapes and `KEY=VALUE` secrets in `.env` style;
//! 3. (strict mode only) long opaque runs and personal folder names.
//!
//! It errs toward over-redaction: a lost word in a transcript costs a human one edit, a leaked key
//! costs more. The token shapes follow the public gitleaks rule set in spirit (MIT,
//! github.com/gitleaks/gitleaks) but are written for this app; none of its code is copied.

use once_cell::sync::Lazy;
use regex::Regex;

pub const MASK: &str = "[REDACTED]";

/// What this process knows to be secret or personal.
#[derive(Debug, Clone, Default)]
pub struct RedactContext {
    /// Values to mask wherever they appear (see `secret_env_values` and `redact::known`).
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

/// How much to mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Text in a file someone may share (a flight cast, a diagnostic bundle, a recorder snapshot):
    /// everything, including long opaque runs and the user's folder name.
    Strict,
    /// Text that other programs and agents keep reading (transcripts, summaries, push, the JSON a
    /// phone parses): known secrets and token shapes only. Commit hashes, ids and paths stay as they
    /// are, because the reader needs them.
    Secrets,
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

/// A part of the input to replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub with: String,
}

/// `(pattern, replacement)`; the replacement may use `${1}` for a capture group.
static SHAPES: Lazy<Vec<(Regex, String)>> = Lazy::new(|| {
    let m = MASK;
    let list: Vec<(&str, String)> = vec![
        (r"\bsk-[A-Za-z0-9_\-]{6,}", m.into()), // OpenAI/Anthropic style (sk-ant-..., sk-proj-...)
        (r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{6,}", m.into()), // GitHub tokens
        (r"github_pat_[A-Za-z0-9_]{6,}", m.into()),
        (r"glpat-[A-Za-z0-9_\-]{10,}", m.into()), // GitLab
        (r"(?:AKIA|ASIA)[0-9A-Z]{12,}", m.into()), // AWS access key ids
        (r"AIza[0-9A-Za-z_\-]{30,}", m.into()),   // Google API keys
        (r"\b(?:sk|pk|rk)_(?:live|test)_[0-9A-Za-z]{10,}", m.into()), // Stripe
        (r"xox[abprs]-[A-Za-z0-9\-]{6,}", m.into()), // Slack
        (r"\bnpm_[A-Za-z0-9]{30,}", m.into()),
        (r"\bhf_[A-Za-z0-9]{30,}", m.into()), // Hugging Face
        (r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}", m.into()), // SendGrid
        (r"eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{4,}\.[A-Za-z0-9_\-]{0,}", m.into()), // JWT
        (r"-----BEGIN [A-Z ]*PRIVATE KEY-----(?s:.*?)(?:-----END [A-Z ]*PRIVATE KEY-----|\z)", m.into()),
        // Credentials inside a URL: scheme://user:password@host
        (r"(?i)\b([a-z][a-z0-9+.\-]*://)[^\s/:@]+:[^\s/@]+@", format!("${{1}}{m}@")),
        (r"(?i)bearer\s+[A-Za-z0-9._~+/=\-]{8,}", m.into()),
        (r"(?i)authorization\s*[:=]\s*(?:basic|token|digest)\s+\S{6,}", m.into()),
        // `.env` style assignment of a secret-named variable, on its own line (quotes allowed).
        (
            r#"(?mi)^[ \t]*(?:export[ \t]+)?([A-Za-z_][A-Za-z0-9_]*(?:key|token|secret|passw(?:or)?d|passwd|credential|private)[A-Za-z0-9_]*)[ \t]*=[ \t]*(?:"[^"\r\n]*"|'[^'\r\n]*'|\S+)"#,
            format!("${{1}}={m}"),
        ),
        // `name=value` / `name: value` anywhere (the value runs to the next space; quotes allowed).
        (
            r#"(?i)(?:api[_-]?key|token|secret|password|passwd)[ \t]*[=:][ \t]*(?:"[^"\r\n]{4,}"|'[^'\r\n]{4,}'|\S{4,})"#,
            m.into(),
        ),
    ];
    list.into_iter()
        .map(|(p, with)| (Regex::new(p).unwrap_or_else(|e| panic!("redaction pattern {p}: {e}")), with))
        .collect()
});

/// A long opaque run: 40+ base64/hex/url-safe characters (at least 30 of them letters or digits, so
/// rules drawn with `=` or `-` survive).
static LONG_RUN: Lazy<Regex> = Lazy::new(|| Regex::new(r"[A-Za-z0-9+/=_\-]{40,}").expect("long run"));

static USER_DIR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)([A-Za-z]:[\\/]+users[\\/]+|/home/|/users/)[^\\/\s]+").expect("user dir"));

/// Case-insensitive (ASCII) occurrences of `needle` in `hay_lower`, which is `hay` lowercased
/// byte-for-byte (`to_ascii_lowercase` keeps every offset).
fn push_exact(spans: &mut Vec<Span>, hay_lower: &str, needle: &str, with: &str) {
    if needle.is_empty() {
        return;
    }
    let needle = needle.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = hay_lower[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        spans.push(Span { start, end, with: with.to_string() });
        from = end;
    }
}

/// Every part of `text` to mask, in order and without overlaps.
pub fn find_spans(text: &str, ctx: &RedactContext, mode: Mode) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let lower = text.to_ascii_lowercase();
    for value in &ctx.env_values {
        if value.len() >= 4 {
            push_exact(&mut spans, &lower, value, MASK);
        }
    }
    for (re, with) in SHAPES.iter() {
        for caps in re.captures_iter(text) {
            let whole = caps.get(0).expect("group 0");
            let mut replacement = String::new();
            caps.expand(with, &mut replacement);
            spans.push(Span { start: whole.start(), end: whole.end(), with: replacement });
        }
    }
    if mode == Mode::Strict {
        for run in LONG_RUN.find_iter(text) {
            if run.as_str().chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 30 {
                spans.push(Span { start: run.start(), end: run.end(), with: MASK.to_string() });
            }
        }
        if let Some(home) = ctx.home.as_deref().filter(|h| h.len() >= 3) {
            let trimmed = home.trim_end_matches(['\\', '/']);
            push_exact(&mut spans, &lower, trimmed, "~");
            push_exact(&mut spans, &lower, &trimmed.replace('\\', "/"), "~");
        }
        // Any other user's folder (or a different spelling of this one) loses its name.
        for caps in USER_DIR.captures_iter(text) {
            let whole = caps.get(0).expect("group 0");
            let mut replacement = String::new();
            caps.expand("${1}USER", &mut replacement);
            spans.push(Span { start: whole.start(), end: whole.end(), with: replacement });
        }
    }
    merge(spans)
}

/// Sort and merge overlapping spans. The earliest (then longest) span keeps its replacement; a span
/// that overlaps it extends it, so a secret is never half masked.
fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start < last.end => {
                if span.end > last.end {
                    last.end = span.end;
                }
                // A mask beats a path rewrite when they meet.
                if span.with == MASK && (last.with == "~" || last.with.ends_with("USER")) {
                    last.with = MASK.to_string();
                }
            }
            _ => out.push(span),
        }
    }
    out
}

/// `text` with `spans` (from `find_spans(text, ..)`) replaced.
pub fn apply(text: &str, spans: &[Span]) -> String {
    if spans.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for span in spans {
        out.push_str(&text[at..span.start]);
        out.push_str(&span.with);
        at = span.end;
    }
    out.push_str(&text[at..]);
    out
}

/// What an email address becomes where one may not be kept (the Agent Wizard's screens, fixtures and records).
pub const EMAIL_MASK: &str = "<email>";

/// Anything that looks like an email address: `local@domain.tld`, a domain with at least one dot and a letters-only
/// last label. So `user@host`, `@scope/pkg` and `git@host:path` with a dotless host are NOT touched, but
/// `git@github.com` is (it errs toward over-redaction). A TUI clips a long row with an ellipsis, so an address cut
/// off at the right edge (`someone@exam...`, `someone@exam` + the ellipsis character) counts too.
static EMAIL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"[\p{L}\p{N}][\p{L}\p{N}._%+\-]*@(?:(?:[\p{L}\p{N}](?:[\p{L}\p{N}\-]*[\p{L}\p{N}])?\.)+\p{L}{2,}|[\p{L}\p{N}.\-]*(?:\x{2026}|\.\.\.))",
    )
    .expect("email regex")
});

/// The made-up address a saved `<email>` is read back as when a profile is checked against a fixture: it is no one's
/// (`.test` is reserved), and a regex for an address matches it like it matches the real one.
pub const EMAIL_EXAMPLE: &str = "user@example.test";

/// `text` with each `<email>` replaced by [`EMAIL_EXAMPLE`]: what a profile reads of a saved screen.
pub fn unmask_emails(text: &str) -> String {
    text.replace(EMAIL_MASK, EMAIL_EXAMPLE)
}

/// `text` with every email address replaced by `<email>`. Not part of [`Mode`]: transcripts and pushes keep
/// emails (a diff or a git log is read by agents); a sink that must not keep one (the wizard's) calls this.
pub fn mask_emails(text: &str) -> String {
    if !text.contains('@') {
        return text.to_string();
    }
    EMAIL.replace_all(text, EMAIL_MASK).into_owned()
}

/// Mask secrets and personal paths in `text`.
pub fn redact(text: &str, ctx: &RedactContext) -> String {
    redact_in(text, ctx, Mode::Strict)
}

pub fn redact_in(text: &str, ctx: &RedactContext, mode: Mode) -> String {
    apply(text, &find_spans(text, ctx, mode))
}

/// Redact a text that is stored as consecutive `pieces` (PTY chunks, typed keystrokes) so that a
/// secret split across pieces is still found: the pieces are joined, spans are found once, and each
/// piece is masked for the part of a span it holds (the piece where a span starts carries the
/// replacement, later pieces lose their share). Returns one string per piece. Every piece must end
/// on a char boundary (decode with `Utf8Carry` first).
pub fn mask_pieces(pieces: &[&str], ctx: &RedactContext, mode: Mode) -> Vec<String> {
    let joined: String = pieces.concat();
    let spans = find_spans(&joined, ctx, mode);
    if spans.is_empty() {
        return pieces.iter().map(|p| p.to_string()).collect();
    }
    let mut out = Vec::with_capacity(pieces.len());
    let mut offset = 0;
    let mut first_span = 0;
    for piece in pieces {
        let (lo, hi) = (offset, offset + piece.len());
        offset = hi;
        while first_span < spans.len() && spans[first_span].end <= lo {
            first_span += 1;
        }
        let mut text = String::with_capacity(piece.len());
        let mut at = lo;
        let mut i = first_span;
        while i < spans.len() && spans[i].start < hi {
            let s = &spans[i];
            let (from, to) = (s.start.max(lo), s.end.min(hi));
            text.push_str(&joined[at..from]);
            if s.start >= lo {
                text.push_str(&s.with);
            }
            at = to;
            i += 1;
        }
        text.push_str(&joined[at..hi]);
        out.push(text);
    }
    out
}

/// Decodes a byte stream that arrives in chunks into text per chunk, carrying an incomplete
/// UTF-8 sequence at a chunk's end over to the next chunk instead of turning it into U+FFFD.
#[derive(Default)]
pub struct Utf8Carry {
    pending: Vec<u8>,
}

impl Utf8Carry {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let keep = incomplete_tail(&self.pending);
        let upto = self.pending.len() - keep;
        let text = String::from_utf8_lossy(&self.pending[..upto]).into_owned();
        self.pending.drain(..upto);
        text
    }

    /// Whatever is left at the end of the stream.
    pub fn finish(&mut self) -> String {
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        text
    }
}

/// Length of a trailing incomplete (but so far well-formed) UTF-8 sequence, 0 if none.
fn incomplete_tail(bytes: &[u8]) -> usize {
    for back in 1..=bytes.len().min(3) {
        let b = bytes[bytes.len() - back];
        if b & 0b1100_0000 == 0b1000_0000 {
            continue; // a continuation byte: keep looking for its lead byte
        }
        let need = if b & 0b1110_0000 == 0b1100_0000 {
            2
        } else if b & 0b1111_0000 == 0b1110_0000 {
            3
        } else if b & 0b1111_1000 == 0b1111_0000 {
            4
        } else {
            return 0;
        };
        return if need > back { back } else { 0 };
    }
    0
}

#[cfg(test)]
pub(crate) fn test_ctx() -> RedactContext {
    RedactContext { env_values: vec!["hunter2-secret".into()], home: Some(r"C:\Users\you-pc".into()) }
}
