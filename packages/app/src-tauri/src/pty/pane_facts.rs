//! What a pane has shown during its whole life that no single screen keeps.
//!
//! - **The model.** A banner names it once and scrolls away; a status row names it only while the agent works, or
//!   clipped at the window's edge. The profile's `[tui.footer_model]` reads whichever of those it was written for,
//!   and a run that starts later (a follow-up, a pane someone else was using) never saw that moment. So every
//!   change of the pane's screen is read once, here, and the name read last is kept for the pane
//!   (`tui::resolve_model` keeps a longer name over a clipped reading of its start).
//! - **Hyperlinks.** An agent that shows a shortened path (`.../x.py`) often draws it as an OSC 8 hyperlink whose
//!   target is the whole path. The terminal emulator keeps no links, so the raw output is scanned for them as it
//!   arrives (`feed`) and the text each link showed is kept with its target (`links_since`), newest last.
//!
//! Both are bounded and forgotten when the pane is taken out of the registry.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};

/// A hyperlink the pane drew: the words it showed and where it pointed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub text: String,
    pub target: String,
}

/// Links kept per pane.
const MAX_LINKS: usize = 400;
/// A link's text or target longer than this is not a path.
const MAX_LINK_BYTES: usize = 2048;
/// Panes remembered at once (the oldest is dropped past this).
const MAX_PANES: usize = 256;

/// Where the pane stood when the task of the current turn was about to be typed.
#[derive(Default, Clone)]
struct TurnMark {
    links: usize,
    /// Rows of the pane's text (scrollback plus screen) at that moment, and the last `MARK_ROWS` of them.
    rows: usize,
    tail: Vec<String>,
}

/// Rows of history kept in a turn mark.
const MARK_ROWS: usize = 600;

#[derive(Default)]
struct Facts {
    turn: TurnMark,
    model: Option<String>,
    scan: OscScan,
    links: VecDeque<Link>,
    /// Links seen since the pane started (never goes down, so a position in it stays valid after old links are dropped).
    link_total: usize,
}

#[derive(Default)]
struct Store {
    panes: HashMap<String, Facts>,
    order: VecDeque<String>,
}

static STORE: Lazy<Mutex<Store>> = Lazy::new(Default::default);

fn with_facts<R>(pane_id: &str, f: impl FnOnce(&mut Facts) -> R) -> R {
    let mut store = STORE.lock();
    if !store.panes.contains_key(pane_id) {
        while store.order.len() >= MAX_PANES {
            if let Some(old) = store.order.pop_front() {
                store.panes.remove(&old);
            }
        }
        store.order.push_back(pane_id.to_string());
        store.panes.insert(pane_id.to_string(), Facts::default());
    }
    f(store.panes.get_mut(pane_id).expect("just inserted"))
}

/// The pane is gone.
pub fn forget(pane_id: &str) {
    let mut store = STORE.lock();
    store.panes.remove(pane_id);
    store.order.retain(|id| id != pane_id);
}

// ---- the model ------------------------------------------------------------------------------------------------------

/// Read the model off `screen` with the pane's profile and remember it for the pane. Returns the pane's model after the
/// look (`None` while no look ever showed one).
pub fn observe_model(pane_id: &str, agent_type: &str, screen: &str) -> Option<String> {
    let readings = crate::tui::profile_for(agent_type).footer_model_readings(screen);
    if readings.is_empty() {
        return model_of(pane_id);
    }
    with_facts(pane_id, |facts| {
        if let Some(model) = crate::tui::resolve_model(facts.model.as_deref(), &readings) {
            facts.model = Some(model);
        }
        facts.model.clone()
    })
}

/// The model any look of this pane's life showed last.
pub fn model_of(pane_id: &str) -> Option<String> {
    STORE.lock().panes.get(pane_id).and_then(|facts| facts.model.clone())
}

/// For tests: the pane remembers `model` as if a look had shown it.
#[cfg(test)]
pub fn remember_model(pane_id: &str, model: &str) {
    with_facts(pane_id, |facts| facts.model = Some(model.to_string()));
}

// ---- hyperlinks ------------------------------------------------------------------------------------------------------

/// Scan a chunk of the pane's raw output for OSC 8 hyperlinks. Cheap when there are none: one byte compare per byte.
pub fn feed(pane_id: &str, bytes: &[u8]) {
    with_facts(pane_id, |facts| {
        if !facts.scan.busy() && !facts.scan.link_open() && !bytes.contains(&0x1b) {
            return;
        }
        facts.scan.feed(bytes);
        for link in facts.scan.done.drain(..) {
            facts.link_total += 1;
            if facts.links.back() == Some(&link) {
                continue;
            }
            facts.links.push_back(link);
            while facts.links.len() > MAX_LINKS {
                facts.links.pop_front();
            }
        }
    });
}

/// A turn is about to begin: remember how many links the pane has drawn and what its text (`full`, scrollback plus
/// screen) holds, so the turn's own links and rows can be told from everything before them.
pub fn mark_turn(pane_id: &str, full: &str) {
    let rows: Vec<&str> = full.lines().collect();
    let tail: Vec<String> = rows[rows.len().saturating_sub(MARK_ROWS)..].iter().map(|r| r.trim_end().to_string()).collect();
    with_facts(pane_id, |facts| {
        facts.turn = TurnMark { links: facts.link_total, rows: rows.len(), tail };
    });
}

/// The links the pane drew since `mark_turn`, oldest first.
pub fn turn_links(pane_id: &str) -> Vec<Link> {
    let mark = STORE.lock().panes.get(pane_id).map_or(0, |facts| facts.turn.links);
    links_since(pane_id, mark)
}

/// The rows of `full` (the pane's text now) that were not there when `mark_turn` ran: the scrollback and screen from
/// the first row that differs from the text of that moment. A turn that cannot be told from the text (the history was
/// wiped or rotated, so what stood at the mark is no longer where it was) gives `None`.
pub fn rows_since_turn_mark(pane_id: &str, full: &str) -> Option<String> {
    let mark = STORE.lock().panes.get(pane_id).map(|facts| facts.turn.clone())?;
    rows_after(&mark, full)
}

fn rows_after(mark: &TurnMark, full: &str) -> Option<String> {
    if mark.tail.is_empty() {
        return None;
    }
    let now: Vec<&str> = full.lines().collect();
    let start = mark.rows.checked_sub(mark.tail.len())?;
    if now.len() < start {
        return None;
    }
    let same = mark
        .tail
        .iter()
        .enumerate()
        .take_while(|(i, old)| now.get(start + i).is_some_and(|row| row.trim_end() == old.as_str()))
        .count();
    if same == 0 {
        return None;
    }
    let from = start + same;
    (from < now.len()).then(|| now[from..].join("\n"))
}

/// How many links the pane has drawn so far: the mark a turn takes before its task is typed.
pub fn link_mark(pane_id: &str) -> usize {
    STORE.lock().panes.get(pane_id).map_or(0, |facts| facts.link_total)
}

/// The links the pane drew after `mark` (a `link_mark`), oldest first.
pub fn links_since(pane_id: &str, mark: usize) -> Vec<Link> {
    let store = STORE.lock();
    let Some(facts) = store.panes.get(pane_id) else { return Vec::new() };
    let dropped = facts.link_total - facts.links.len();
    let skip = mark.saturating_sub(dropped);
    facts.links.iter().skip(skip).cloned().collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum State {
    #[default]
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

/// A byte-wise reader of terminal output that keeps only OSC 8 hyperlinks and the plain text between a link's start and
/// end. It follows escape sequences across chunk boundaries and ignores every other escape.
#[derive(Default)]
struct OscScan {
    state: State,
    payload: Vec<u8>,
    overflow: bool,
    open: Option<(String, Vec<u8>)>,
    done: VecDeque<Link>,
}

impl OscScan {
    fn busy(&self) -> bool {
        self.state != State::Ground
    }

    fn link_open(&self) -> bool {
        self.open.is_some()
    }

    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.step(b);
        }
    }

    fn step(&mut self, b: u8) {
        match self.state {
            State::Ground => match b {
                0x1b => self.state = State::Esc,
                _ => self.text(b),
            },
            State::Esc => match b {
                b']' => {
                    self.state = State::Osc;
                    self.payload.clear();
                    self.overflow = false;
                }
                b'[' => self.state = State::Csi,
                0x1b => {}
                _ => self.state = State::Ground,
            },
            State::Csi => {
                if (0x40..=0x7e).contains(&b) {
                    self.state = State::Ground;
                }
            }
            State::Osc => match b {
                0x07 => {
                    self.end_osc();
                    self.state = State::Ground;
                }
                0x1b => self.state = State::OscEsc,
                _ => {
                    if self.payload.len() < MAX_LINK_BYTES {
                        self.payload.push(b);
                    } else {
                        self.overflow = true;
                    }
                }
            },
            State::OscEsc => {
                self.end_osc();
                self.state = State::Ground;
                if b != b'\\' {
                    // `ESC` + something else: that is the start of the next escape
                    self.step(0x1b);
                    self.step(b);
                }
            }
        }
    }

    /// A printable byte drawn while a link is open belongs to its text.
    fn text(&mut self, b: u8) {
        if let Some((_, text)) = &mut self.open {
            match b {
                b'\r' | b'\n' => {}
                0x00..=0x1f => {}
                _ if text.len() < MAX_LINK_BYTES => text.push(b),
                _ => {}
            }
        }
    }

    fn end_osc(&mut self) {
        let payload = std::mem::take(&mut self.payload);
        if self.overflow {
            self.overflow = false;
            self.open = None;
            return;
        }
        let Some(rest) = payload.strip_prefix(b"8;") else { return };
        // `8 ; params ; URI`: the parameters (id=...) come first
        let uri = match rest.iter().position(|b| *b == b';') {
            Some(at) => &rest[at + 1..],
            None => return,
        };
        if uri.is_empty() {
            if let Some((target, text)) = self.open.take() {
                let text = String::from_utf8_lossy(&text).trim().to_string();
                if !text.is_empty() && !target.is_empty() {
                    self.done.push_back(Link { text, target });
                }
            }
        } else {
            self.open = Some((String::from_utf8_lossy(uri).into_owned(), Vec::new()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links_of(chunks: &[&[u8]]) -> Vec<Link> {
        let mut scan = OscScan::default();
        for chunk in chunks {
            scan.feed(chunk);
        }
        scan.done.into_iter().collect()
    }

    #[test]
    fn a_hyperlink_is_its_visible_text_and_its_target() {
        let out = links_of(&[b"Edited \x1b]8;;file:///C:/p/src/x.py\x1b\\.../x.py\x1b]8;;\x1b\\ (2 lines)"]);
        assert_eq!(out, vec![Link { text: ".../x.py".into(), target: "file:///C:/p/src/x.py".into() }]);
    }

    #[test]
    fn bel_terminated_links_with_parameters_and_styling_inside_work_and_chunks_may_split_anywhere() {
        let whole: &[u8] = b"a \x1b]8;id=7;file:///tmp/a/b.txt\x07\x1b[4m.../b.txt\x1b[24m\x1b]8;;\x07 b";
        for cut in 0..whole.len() {
            let out = links_of(&[&whole[..cut], &whole[cut..]]);
            assert_eq!(out, vec![Link { text: ".../b.txt".into(), target: "file:///tmp/a/b.txt".into() }], "split at {cut}");
        }
    }

    #[test]
    fn text_with_no_link_and_other_osc_sequences_make_none() {
        assert!(links_of(&[b"plain \x1b[31mred\x1b[0m text \x1b]0;a title\x07 more"]).is_empty());
        // an unclosed link is not a link yet
        assert!(links_of(&[b"\x1b]8;;file:///a/b\x1b\\half"]).is_empty());
    }

    #[test]
    fn the_pane_keeps_links_after_a_mark_and_forgets_with_the_pane() {
        let id = "pane-facts-links";
        forget(id);
        feed(id, b"\x1b]8;;file:///a/one.rs\x1b\\.../one.rs\x1b]8;;\x1b\\\n");
        let mark = link_mark(id);
        assert_eq!(mark, 1);
        feed(id, b"x \x1b]8;;file:///a/two.rs\x1b\\.../two.rs\x1b]8;;\x1b\\\n");
        assert_eq!(links_since(id, mark), vec![Link { text: ".../two.rs".into(), target: "file:///a/two.rs".into() }]);
        assert_eq!(links_since(id, 0).len(), 2);
        forget(id);
        assert!(links_since(id, 0).is_empty());
    }

    #[test]
    fn the_rows_of_a_turn_start_at_the_first_row_that_changed_since_the_mark() {
        let before = "banner\nold reply\n> \n\n";
        let id = "pane-facts-rows";
        forget(id);
        mark_turn(id, before);
        // the box now holds the task, then the reply scrolls the old text up; the old rows stay in place above
        let now = "banner\nold reply\n> do it\nreply: long\nmore\nand more\n> \n";
        assert_eq!(rows_since_turn_mark(id, now).as_deref(), Some("> do it\nreply: long\nmore\nand more\n> "));
        // nothing changed: no rows
        assert_eq!(rows_since_turn_mark(id, before), None);
        // history wiped: what stood at the mark is not there any more
        assert_eq!(rows_since_turn_mark(id, "something else entirely\nreply\n"), None);
        forget(id);
    }

    #[test]
    fn a_clipped_reading_never_replaces_the_longer_name_the_pane_showed() {
        let id = "pane-facts-model";
        forget(id);
        remember_model(id, "zeta-banner-pro");
        assert_eq!(model_of(id).as_deref(), Some("zeta-banner-pro"));
        forget(id);
        assert_eq!(model_of(id), None);
    }
}
