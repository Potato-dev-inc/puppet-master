//! The generic limit detector: one screen in, at most one [`LimitHit`] out, driven only by a
//! profile's `[limits]` table.
//!
//! False-match guards (the design's four, `DESIGN-RELAY.md` 1.4; the fourth, holding still, is the
//! tracker's, `watch.rs`):
//!
//! 1. The match must lie in the last `tail_lines` non-blank rows of the screen. The screen text is
//!    the emulator's, so a row the terminal soft-wrapped is already one row; rows the agent wrapped
//!    itself (Codex at 52 columns) are joined by the rules' `\s+`.
//! 2. A turn must not be running (`busy`, the profile's own `busy_marker`) for a usage limit, a
//!    menu or an unavailable plan. A transient retry and a warning are shown WHILE working.
//! 3. The matched row must not sit inside a transcript block (the agent's own reply or tool row, the
//!    person's prompt echo: `block_markers`) unless it is a notice row (`notice_glyph`): an agent
//!    quoting the limit sentence, or a person pasting it, is not a limit.

use super::limits::{CompiledLimits, CompiledRule};
use super::reset::{parse_reset, squash, Clock};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    /// The plan window is used up.
    Usage,
    /// The vendor throttles; the CLI retries by itself.
    Transient,
    /// The CLI gave up retrying.
    TransientFinal,
    /// "Less than 25% left".
    Warning,
    /// The plan does not include the agent.
    Unavailable,
    /// The vendor's own limit menu is open (it may offer paid choices). Never answered.
    Menu,
}

impl LimitKind {
    /// The agent is stopped until a person (or the reset) acts, so no task may be typed into its
    /// pane: a usage limit, an unavailable plan, or the vendor's own menu.
    pub fn stops_typing(self) -> bool {
        matches!(self, Self::Usage | Self::Unavailable | Self::Menu)
    }
}

/// How sure the detector is, ordinal on purpose (no floats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitEvidence {
    /// On screen now.
    Seen,
    /// On screen and unchanged for the profile's `stable_ms`.
    Stable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LimitHit {
    pub kind: LimitKind,
    pub rule_id: String,
    /// "session limit", "weekly limit", "gpt-5.5": what ran out.
    pub window: Option<String>,
    /// The reset as the screen printed it, whitespace squashed ("5pm (Asia/Taipei)").
    pub reset_text: Option<String>,
    /// The reset as a moment (epoch ms); `None` when unprinted, unreadable or in a foreign zone.
    pub reset_at_ms: Option<u64>,
    /// The CLI shows it will carry on by itself at the reset (its own "continue automatically").
    pub self_resume_armed: bool,
    /// The vendor's limit menu is open next to the notice.
    pub menu_open: bool,
    pub evidence: LimitEvidence,
    /// The profile's `stable_ms`: how long the match has to hold.
    pub stable_ms: u64,
    /// Short hash of the limit's identity, for the flight recorder and for "is this the same limit".
    pub screen_sha: String,
}

/// One rule's best match: where it is and what it captured.
struct Found {
    rule: usize,
    window: Option<String>,
    reset: Option<String>,
    text: String,
}

/// Rows of the screen with trailing blanks dropped.
fn rows(screen: &str) -> Vec<&str> {
    let mut rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    while rows.last().is_some_and(|r| r.trim().is_empty()) {
        rows.pop();
    }
    rows
}

fn indent(row: &str) -> usize {
    row.len() - row.trim_start().len()
}

/// The first row of the tail that holds the last `tail_lines` non-blank rows (0 when there are fewer).
fn tail_start(all: &[&str], tail_lines: usize) -> usize {
    let mut seen = 0;
    for (index, row) in all.iter().enumerate().rev() {
        if !row.trim().is_empty() {
            seen += 1;
            if seen == tail_lines {
                return index;
            }
        }
    }
    0
}

impl CompiledLimits {
    /// Guard 3. The block a row belongs to is opened by the nearest row above it that starts in
    /// column 0. Inside a block opened by one of the profile's `block_markers`, only a row that
    /// starts with the `notice_glyph` can be a notice. Rows under the input box (below the last
    /// full-width rule) are the CLI's own footer, never a transcript block.
    fn inside_foreign_block(&self, rows: &[&str], row: usize) -> bool {
        if self.table.block_markers.is_empty() {
            return false;
        }
        let is_rule = |r: &str| r.trim_start().starts_with("\u{2500}\u{2500}\u{2500}");
        if rows.iter().rposition(|r| is_rule(r)).is_some_and(|last_rule| row > last_rule) {
            return false;
        }
        let line = rows[row];
        let opener = if indent(line) == 0 {
            line
        } else {
            match rows[..row].iter().rev().find(|r| !r.trim().is_empty() && indent(r) == 0) {
                Some(opener) => opener,
                None => return false,
            }
        };
        let opener = opener.trim_start();
        if !self.table.block_markers.iter().any(|m| opener.starts_with(m.as_str())) {
            return false;
        }
        let notice = self.table.notice_glyph.as_deref().is_some_and(|glyph| line.trim_start().starts_with(glyph));
        !notice
    }

    /// The last match of `rule` in `text` that survives guard 3.
    fn find(&self, rule: &CompiledRule, index: usize, tail: &str, tail_rows: &[&str], all_rows: &[&str], first_tail_row: usize) -> Option<Found> {
        let mut best = None;
        for caps in rule.regex.captures_iter(tail) {
            let whole = caps.get(0)?;
            // Where the notice really starts: leading blanks and newlines a rule may swallow do not
            // count.
            let lead = whole.as_str().len() - whole.as_str().trim_start().len();
            let start = whole.start() + lead;
            let tail_row = tail[..start].matches('\n').count();
            let row = first_tail_row + tail_row;
            if tail_row >= tail_rows.len() || self.inside_foreign_block(all_rows, row) {
                continue;
            }
            let group = |name: &str| caps.name(name).map(|m| squash(m.as_str())).filter(|s| !s.is_empty());
            let mut reset = group("reset");
            // The agent wrapped the zone onto its own row: "5pm" / "(Asia/Taipei)".
            if let (Some(text), Some(next)) = (reset.as_mut(), all_rows.get(first_tail_row + tail[..whole.end()].matches('\n').count() + 1)) {
                let next = next.trim();
                if !text.ends_with(')') && next.starts_with('(') && next.ends_with(')') && !next.contains(' ') {
                    text.push(' ');
                    text.push_str(next);
                }
            }
            best = Some(Found { rule: index, window: group("window"), reset, text: squash(whole.as_str()) });
        }
        best
    }

    /// What the screen says about a limit, if anything. `busy` is the profile's `busy_marker`.
    pub fn detect(&self, screen: &str, busy: Option<bool>, clock: &dyn Clock) -> Option<LimitHit> {
        let all = rows(screen);
        // Guard 1 counts NON-BLANK rows: a CLI that pins its input box to the bottom of the screen
        // leaves blank rows between the history and the box, and those must not push a notice out.
        let first = tail_start(&all, self.table.tail_lines);
        let tail_rows = &all[first..];
        let tail = tail_rows.join("\n");
        let working = busy == Some(true);
        let find_all = |rules: &[CompiledRule]| -> Vec<Found> {
            rules.iter().enumerate().filter_map(|(i, r)| self.find(r, i, &tail, tail_rows, &all, first)).collect()
        };

        let menu = if working { Vec::new() } else { find_all(&self.menu) };
        let usage = if working { Vec::new() } else { find_all(&self.usage) };
        if !usage.is_empty() {
            let armed = usage.iter().any(|f| self.usage[f.rule].armed);
            let lead = usage.iter().find(|f| self.usage[f.rule].armed).unwrap_or(&usage[0]);
            let window = usage.iter().find_map(|f| f.window.clone());
            let reset = lead.reset.clone().or_else(|| usage.iter().find_map(|f| f.reset.clone()));
            return Some(self.hit(LimitKind::Usage, &self.usage[lead.rule].id, window, reset, armed, !menu.is_empty(), &lead.text, clock));
        }
        if let Some(found) = menu.first() {
            return Some(self.hit(LimitKind::Menu, &self.menu[found.rule].id, None, None, false, true, &found.text, clock));
        }
        let simple = |kind: LimitKind, rules: &[CompiledRule], guard_busy: bool| -> Option<LimitHit> {
            if guard_busy && working {
                return None;
            }
            let found = find_all(rules).into_iter().next()?;
            Some(self.hit(kind, &rules[found.rule].id, found.window, found.reset, false, false, &found.text, clock))
        };
        simple(LimitKind::Unavailable, &self.unavailable, true)
            .or_else(|| simple(LimitKind::TransientFinal, &self.transient_final, true))
            .or_else(|| simple(LimitKind::Transient, &self.transient, false))
            .or_else(|| simple(LimitKind::Warning, &self.warning, false))
    }

    #[allow(clippy::too_many_arguments)]
    fn hit(
        &self,
        kind: LimitKind,
        rule_id: &str,
        window: Option<String>,
        reset_text: Option<String>,
        self_resume_armed: bool,
        menu_open: bool,
        matched: &str,
        clock: &dyn Clock,
    ) -> LimitHit {
        let reset_at_ms = reset_text
            .as_deref()
            .and_then(|text| parse_reset(text, &self.reset_formats, clock))
            .and_then(|ms| u64::try_from(ms).ok());
        // "Is this the same limit": by what ran out and when it resets, not by which line shows it, so
        // the agent adding its "continuing automatically" line (or the notice scrolling) does not make
        // a new limit. A limit that names neither falls back to its own matched text.
        let identity = if window.is_some() || reset_text.is_some() {
            format!("{kind:?}|{}|{}", window.as_deref().unwrap_or(""), reset_text.as_deref().unwrap_or(""))
        } else {
            format!("{kind:?}|{rule_id}|{matched}")
        };
        let sha = Sha256::digest(identity.as_bytes());
        let screen_sha: String = sha.iter().take(6).map(|b| format!("{b:02x}")).collect();
        LimitHit {
            kind,
            rule_id: rule_id.to_string(),
            window,
            reset_text,
            reset_at_ms,
            self_resume_armed,
            menu_open,
            evidence: LimitEvidence::Seen,
            stable_ms: self.table.stable_ms,
            screen_sha,
        }
    }
}
