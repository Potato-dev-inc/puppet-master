//! Redaction for what the bridge fans out to phones and other clients (`bridge::sse::push_sse`).
//!
//! The terminal stream is the live terminal the person is looking at, and a few events are
//! structure the client parses by value (pane lists, status, geometry, settings). Those pass
//! untouched. Every other event is JSON: its string fields are redacted in [`Mode::Secrets`]
//! (known secrets and token shapes only), so ids, paths and hashes survive and the JSON stays valid.

use super::{known, rules, Mode, Scrub};

/// Events that are not redacted, with the reason.
const PASS_THROUGH: &[&str] = &[
    "terminal",    // the live terminal
    "pane-resize", // geometry
    "pane-status", // an enum
    "panes",       // the pane list (ids, sizes, titles the person chose)
    "settings",    // already public settings
];

/// `event: NAME\ndata: JSON\n\n` with the JSON's strings redacted. Returns the input unchanged when
/// nothing was masked, so unrelated events keep their exact bytes.
pub fn sse_frame(frame: String) -> String {
    let Some(rest) = frame.strip_prefix("event: ") else { return frame };
    let Some((name, data)) = rest.split_once("\ndata: ") else { return frame };
    if PASS_THROUGH.contains(&name) {
        return frame;
    }
    let (body, tail) = match data.strip_suffix("\n\n") {
        Some(body) => (body, "\n\n"),
        None => (data, ""),
    };
    let ctx = known::context();
    let rebuilt = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(mut value) => {
            let mut changed = false;
            value.scrub(&mut |s| {
                let out = rules::redact_in(s, &ctx, Mode::Secrets);
                changed |= out != s;
                out
            });
            if !changed {
                return frame;
            }
            serde_json::to_string(&value).unwrap_or_else(|_| rules::MASK.to_string())
        }
        Err(_) => {
            let out = rules::redact_in(body, &ctx, Mode::Secrets);
            if out == body {
                return frame;
            }
            out
        }
    };
    format!("event: {name}\ndata: {rebuilt}{tail}")
}
