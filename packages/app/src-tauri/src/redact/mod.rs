//! One place that decides what text may leave the live terminal.
//!
//! Every sink that saves or sends text (transcripts, turn summaries, push payloads, flight casts,
//! diagnostic bundles, the JSON events a phone receives) takes [`Redacted`] text. A `Redacted<T>`
//! can only be made here (`text`, `scrub`, `json`), so a new sink that asks for it cannot be handed
//! a raw `String` by accident:
//!
//! ```compile_fail
//! // A raw string does not compile where a redacted one is required.
//! puppet_master_app_lib::agent_runs::transcript::append("p", "op", "stdout", "sk-ant-api03-secret");
//! ```
//!
//! ```no_run
//! // The same call compiles once the text went through `redact`.
//! use puppet_master_app_lib::{agent_runs::transcript, redact};
//! transcript::append("p", "op", "stdout", &redact::text("hello"));
//! ```
//!
//! The live terminal the person is looking at is never redacted; only what is saved or sent on.
//!
//! - `rules`: the engine (spans, patterns, `mask_pieces`), promoted from the recorder.
//! - `known`: the secrets this process knows by value (env, credential store, toolbelt, OpenCode).
//! - `fanout`: the JSON-safe pass for events sent to phones.
//!
//! Two strengths: [`text`] (secrets only, for text other programs keep reading) and [`strict`]
//! (also long opaque runs and personal folder names, for files that may be shared).

pub mod fanout;
pub mod known;
pub mod rules;

#[cfg(test)]
mod sink_tests;
#[cfg(test)]
mod tests;

pub use rules::{
    apply, find_spans, mask_emails, mask_pieces, redact, redact_in, secret_env_values, Mode, RedactContext, Span, Utf8Carry,
    unmask_emails, EMAIL_EXAMPLE, EMAIL_MASK, MASK,
};

use serde::Serialize;

/// Text (or data made of text) that went through redaction. The field is private: the only ways to
/// get one are the functions in this module.
#[derive(Clone, PartialEq, Eq)]
pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    pub fn get(&self) -> &T {
        &self.0
    }
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl Redacted<String> {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redacted({:?})", self.0)
    }
}

impl std::fmt::Display for Redacted<String> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Serializes as the inner value, so a redacted struct can still go to disk as JSON.
impl<T: Serialize> Serialize for Redacted<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Redact text that programs and agents keep reading (transcripts, summaries, push, JSON fields):
/// known secrets and token shapes, see [`Mode::Secrets`]. Commit hashes, ids and paths stay.
pub fn text(raw: &str) -> Redacted<String> {
    text_with(raw, &known::context())
}

pub fn text_with(raw: &str, ctx: &RedactContext) -> Redacted<String> {
    Redacted(rules::redact_in(raw, ctx, Mode::Secrets))
}

/// Redact text that goes into a file someone may share (flight casts, diagnostic bundles): the
/// secrets of [`text`] plus long opaque runs and the user's folder name, see [`Mode::Strict`].
pub fn strict(raw: &str) -> Redacted<String> {
    strict_with(raw, &known::context())
}

pub fn strict_with(raw: &str, ctx: &RedactContext) -> Redacted<String> {
    Redacted(rules::redact_in(raw, ctx, Mode::Strict))
}

/// Data made of text that can be rewritten in place (see [`Scrub`]), in [`Mode::Secrets`].
pub fn scrub<T: Scrub>(mut value: T) -> Redacted<T> {
    let ctx = known::context();
    value.scrub(&mut |s| rules::redact_in(s, &ctx, Mode::Secrets));
    Redacted(value)
}

/// A JSON value with every string redacted (keys stay as they are), in [`Mode::Secrets`].
pub fn json(value: serde_json::Value) -> Redacted<serde_json::Value> {
    scrub(value)
}

/// Text built only from program constants and numbers (a fixed label, a byte count): nothing in it
/// can be a secret. Prefer [`text`]; this is for hot paths and for sinks fed only with constants.
pub fn trusted_constant(literal: &'static str) -> Redacted<String> {
    Redacted(literal.to_string())
}

/// Rewrite every string inside a value.
pub trait Scrub {
    fn scrub(&mut self, f: &mut dyn FnMut(&str) -> String);
}

impl Scrub for String {
    fn scrub(&mut self, f: &mut dyn FnMut(&str) -> String) {
        *self = f(self);
    }
}

impl<T: Scrub> Scrub for Option<T> {
    fn scrub(&mut self, f: &mut dyn FnMut(&str) -> String) {
        if let Some(inner) = self {
            inner.scrub(f);
        }
    }
}

impl<T: Scrub> Scrub for Vec<T> {
    fn scrub(&mut self, f: &mut dyn FnMut(&str) -> String) {
        for item in self {
            item.scrub(f);
        }
    }
}

impl Scrub for serde_json::Value {
    fn scrub(&mut self, f: &mut dyn FnMut(&str) -> String) {
        match self {
            serde_json::Value::String(s) => *s = f(s),
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| v.scrub(f)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| v.scrub(f)),
            _ => {}
        }
    }
}

/// Redact text stored as consecutive pieces (PTY chunks, typed keystrokes) so that a secret split
/// across pieces is still masked, in [`Mode::Strict`]. See [`rules::mask_pieces`].
pub fn strict_pieces(pieces: &[&str]) -> Vec<Redacted<String>> {
    rules::mask_pieces(pieces, &known::context(), Mode::Strict).into_iter().map(Redacted).collect()
}

/// A piece of a text assembled from redacted parts and program-written literals.
pub enum Part<'a> {
    /// Text the program wrote itself (JSON punctuation, a number, a fixed label). Never user, agent
    /// or file content: use `Red` or `Quoted` for that.
    Lit(&'a str),
    /// Redacted text, inserted as it is.
    Red(&'a Redacted<String>),
    /// Redacted text as a JSON string, quotes and escapes included.
    Quoted(&'a Redacted<String>),
}

/// Join redacted parts with literals into one redacted text (a JSON line, a file body).
pub fn compose(parts: &[Part<'_>]) -> Redacted<String> {
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Lit(s) => out.push_str(s),
            Part::Red(r) => out.push_str(r.as_str()),
            Part::Quoted(r) => out.push_str(&serde_json::to_string(r.as_str()).unwrap_or_else(|_| "\"\"".into())),
        }
    }
    Redacted(out)
}

/// A JSON value with every string redacted in [`Mode::Strict`] (for files that may be shared).
pub fn json_strict(mut value: serde_json::Value) -> Redacted<serde_json::Value> {
    let ctx = known::context();
    value.scrub(&mut |s| rules::redact_in(s, &ctx, Mode::Strict));
    Redacted(value)
}

impl Redacted<serde_json::Value> {
    /// The value as compact JSON text. Escaping cannot reveal what redaction removed.
    pub fn to_text(&self) -> Redacted<String> {
        Redacted(serde_json::to_string(&self.0).unwrap_or_default())
    }

    /// The value as indented JSON text.
    pub fn to_pretty_text(&self) -> Redacted<String> {
        Redacted(serde_json::to_string_pretty(&self.0).unwrap_or_default())
    }
}
