//! A ring snapshot as an asciicast v2 file (https://docs.asciinema.org/manual/asciicast/v2/).
//!
//! - `o` events are PTY output, `i` events are input, `r` events are resizes.
//! - Who wrote an input is a marker in front of it: `[t, "m", "w=run_task"]` (the standard marker
//!   event, so any player ignores or shows it). A marker with a decision reads `pm:<text>`.
//! - Everything is redacted before it is written (`redact::strict_pieces`): the pieces of each
//!   stream are masked together, so a key split over two PTY chunks is still caught.

use super::ring::{Kind, Snapshot};
use super::InputTag;
use crate::redact::{self, compose, Part, Redacted, Utf8Carry};

/// The cast for `snap`. `title` and `closing_note` describe why it was taken ("PROMPT_NOT_SUBMITTED").
pub fn render(snap: &Snapshot, title: &str, closing_note: Option<&str>) -> Redacted<String> {
    // 1. Decode each stream to text, keeping a split UTF-8 character together.
    let mut out_text: Vec<String> = Vec::new();
    let mut in_text: Vec<String> = Vec::new();
    let (mut carry_out, mut carry_in) = (Utf8Carry::default(), Utf8Carry::default());
    // Which text each event uses.
    let mut slot: Vec<usize> = Vec::with_capacity(snap.events.len());
    for event in &snap.events {
        match event.kind {
            Kind::Output => {
                slot.push(out_text.len());
                out_text.push(carry_out.push(snap.data(event)));
            }
            Kind::Input => {
                slot.push(in_text.len());
                in_text.push(carry_in.push(snap.data(event)));
            }
            Kind::Resize | Kind::Marker => slot.push(0),
        }
    }
    if let Some(last) = out_text.last_mut() {
        last.push_str(&carry_out.finish());
    }
    if let Some(last) = in_text.last_mut() {
        last.push_str(&carry_in.finish());
    }

    // 2. Redact each stream as a whole.
    let out_refs: Vec<&str> = out_text.iter().map(String::as_str).collect();
    let in_refs: Vec<&str> = in_text.iter().map(String::as_str).collect();
    let out_red = redact::strict_pieces(&out_refs);
    let in_red = redact::strict_pieces(&in_refs);

    // 3. Lines.
    let t0 = snap.events.first().map(|e| e.t_us).unwrap_or(0);
    let secs = |t_us: u64| format!("{:.6}", t_us.saturating_sub(t0) as f64 / 1_000_000.0);
    let timestamp = (snap.start_unix_ms + (t0 / 1000) as i64) / 1000;
    let title_red = redact::strict(title);
    let header = compose(&[
        Part::Lit(&format!(
            "{{\"version\":2,\"width\":{},\"height\":{},\"timestamp\":{},\"env\":{{\"TERM\":\"xterm-256color\"}},\"title\":",
            snap.base_cols.max(1),
            snap.base_rows.max(1),
            timestamp
        )),
        Part::Quoted(&title_red),
        Part::Lit("}\n"),
    ]);
    let mut lines: Vec<Redacted<String>> = vec![header];

    let marker = |t: &str, text: &Redacted<String>| {
        compose(&[Part::Lit("["), Part::Lit(t), Part::Lit(",\"m\","), Part::Quoted(text), Part::Lit("]\n")])
    };
    if snap.truncated {
        let note = redact::trusted_constant("pm:older activity was dropped to keep the recording small");
        lines.push(marker("0.000000", &note));
    }
    let mut last_tag: Option<InputTag> = None;
    for (event, slot) in snap.events.iter().zip(&slot) {
        let t = secs(event.t_us);
        match event.kind {
            Kind::Output => lines.push(compose(&[
                Part::Lit("["),
                Part::Lit(&t),
                Part::Lit(",\"o\","),
                Part::Quoted(&out_red[*slot]),
                Part::Lit("]\n"),
            ])),
            Kind::Input => {
                let tag = InputTag::from_u32(event.aux);
                if last_tag != Some(tag) {
                    last_tag = Some(tag);
                    let w = redact::trusted_constant(match tag {
                        InputTag::User => "w=user",
                        InputTag::Phone => "w=phone",
                        InputTag::RunTask => "w=run_task",
                        InputTag::GateDismiss => "w=gate_dismiss",
                        InputTag::Answer => "w=answer",
                        InputTag::Nudge => "w=nudge",
                        InputTag::Agent => "w=agent",
                        InputTag::Shell => "w=shell",
                    });
                    lines.push(marker(&t, &w));
                }
                lines.push(compose(&[
                    Part::Lit("["),
                    Part::Lit(&t),
                    Part::Lit(",\"i\","),
                    Part::Quoted(&in_red[*slot]),
                    Part::Lit("]\n"),
                ]));
            }
            Kind::Resize => {
                let size = format!("{}x{}", event.aux >> 16, event.aux & 0xffff);
                lines.push(compose(&[
                    Part::Lit("["),
                    Part::Lit(&t),
                    Part::Lit(",\"r\",\""),
                    Part::Lit(&size),
                    Part::Lit("\"]\n"),
                ]));
            }
            Kind::Marker => {
                let text = redact::strict(&format!("pm:{}", String::from_utf8_lossy(snap.data(event))));
                lines.push(marker(&t, &text));
            }
        }
    }
    if let Some(note) = closing_note {
        let last_t = snap.events.last().map(|e| e.t_us).unwrap_or(t0);
        let text = redact::strict(&format!("pm:{note}"));
        lines.push(marker(&secs(last_t), &text));
    }
    let parts: Vec<Part<'_>> = lines.iter().map(Part::Red).collect();
    compose(&parts)
}

