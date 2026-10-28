//! Replays a pane's flight-recorder ring into sampled screens (spec 3.4).
//!
//! The ring holds the bytes the PTY printed. They are fed into a `vt100` screen (the emulator the
//! app itself uses) and a frame is kept every `KEYFRAME_MS` of clip time while the screen changes.
//! Real pauses longer than `GAP_MAX_MS` are squeezed, and a long turn is sped up so the clip stays
//! at most `REPLAY_MAX_MS` long. Only output and resize events are replayed: typed input (which can
//! be a secret) and markers never reach a frame.
//!
//! The frames are raw. `scrub` redacts them before anything leaves Rust.

use super::model::{GAP_MAX_MS, MAX_FRAMES, REPLAY_MAX_MS};
use crate::flight::ring::{Event, Kind, Snapshot};

/// Real time kept before the turn starts, so the first screen is not cut.
const PAD_BEFORE_MS: i64 = 300;
/// Real time kept after the turn ends, at least.
const PAD_AFTER_MS: i64 = 700;
/// After the turn's end the replay goes on while output keeps coming at most this far apart. A pane
/// rests (its status turns idle) a moment before its last screen is drawn, and the clip used to stop on
/// the spinner line without the agent's answer.
const SETTLE_QUIET_MS: i64 = 1_200;
/// The most the replay runs past the end of the turn while it waits for the output to go quiet.
const SETTLE_MAX_MS: i64 = 8_000;
/// Clip time between two keyframes.
pub const KEYFRAME_MS: u32 = 100;
/// Clip time between two keyframes when 100 ms would pass `MAX_FRAMES`.
pub const SLOW_KEYFRAME_MS: u32 = 200;
/// Shortest turn (real time) that is worth a replay.
const MIN_WINDOW_MS: u64 = 2_000;
/// Largest grid size taken from a resize event (bounds the emulator's memory).
const MAX_GRID: u16 = 1_000;

/// Style bits of a cell (the `s` of a run in the plan).
pub const BOLD: u8 = 1;
pub const DIM: u8 = 2;
pub const INVERSE: u8 = 4;
pub const UNDERLINE: u8 = 8;
/// The background is not the default colour.
pub const HIGHLIGHT: u8 = 16;

/// One screen cell. `ch` is `" "` for a blank, `""` for the right half of a wide character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCell {
    pub ch: String,
    pub style: u8,
}

pub type RawRow = Vec<RawCell>;

/// One screen at one clip time, before redaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    pub at_ms: u32,
    pub rows: Vec<RawRow>,
    /// `(row, col)`, or `None` when the cursor is hidden.
    pub cursor: Option<(u16, u16)>,
    /// Whether each row continues on the next one (a long line the terminal wrapped).
    pub wrapped: Vec<bool>,
}

impl RawFrame {
    fn is_blank(&self) -> bool {
        self.rows.iter().all(|row| row.iter().all(|cell| cell.ch.is_empty() || cell.ch == " "))
    }
}

/// Maps real unix time to clip time. Built from the replayed events: each real gap is squeezed to
/// at most `GAP_MAX_MS`, then the whole replay is divided by `speed`.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeMap {
    /// `(unix ms, squeezed ms)` at the start, at each replayed event and at the end. Ascending.
    knots: Vec<(i64, u64)>,
    speed: f64,
    length_ms: u32,
}

impl TimeMap {
    /// Clip time (ms) of a real unix time. Before the replay starts it is 0; after it ends, the
    /// length of the clip.
    pub fn clip_ms(&self, unix_ms: u64) -> u32 {
        self.clip_at(unix_ms.min(i64::MAX as u64) as i64)
    }

    /// Length of the replay in clip milliseconds.
    #[allow(dead_code)] // part of the time map's API; `plan` reads the length from `Replayed`
    pub fn length_ms(&self) -> u32 {
        self.length_ms
    }

    fn clip_at(&self, t: i64) -> u32 {
        let squeezed = self.squeezed_at(t);
        ((squeezed / self.speed).round() as u32).min(self.length_ms)
    }

    /// Squeezed time at real time `t`, linear between the knots.
    fn squeezed_at(&self, t: i64) -> f64 {
        let (Some(first), Some(last)) = (self.knots.first(), self.knots.last()) else {
            return 0.0;
        };
        if t <= first.0 {
            return first.1 as f64;
        }
        if t >= last.0 {
            return last.1 as f64;
        }
        // `knots[i - 1].0 <= t < knots[i].0`, so the gap between them is never zero.
        let i = self.knots.partition_point(|knot| knot.0 <= t);
        let (a, b) = (self.knots[i - 1], self.knots[i]);
        let ratio = (t - a.0) as f64 / (b.0 - a.0) as f64;
        a.1 as f64 + ratio * (b.1 as f64 - a.1 as f64)
    }
}

/// A replayed turn: the frames and what they mean.
#[derive(Debug, Clone)]
pub struct Replayed {
    /// The largest grid of any frame. Every frame is padded to it.
    pub cols: u16,
    pub rows: u16,
    pub frames: Vec<RawFrame>,
    /// Active time over clip length, rounded, at least 1 ("Sped up Nx"). Active time is the window
    /// minus the time that squeezing removed from its pauses, so a long idle pause does not inflate it.
    pub speed_x: u32,
    /// Raw speed: window length over clip length, rounded, at least 1. Pauses count in full. Not
    /// shown by default; kept for a plan that needs the unsqueezed ratio.
    #[allow(dead_code)]
    pub raw_speed_x: u32,
    pub length_ms: u32,
    /// The ring holds at least half of the turn. When false, show the steps instead.
    pub covered: bool,
    /// The ring dropped older bytes. The caller adds a note when `covered` is also true.
    pub truncated: bool,
    pub time_map: TimeMap,
}

/// Replays the turn `[start_ms, end_ms]` (unix ms) with keyframes every `KEYFRAME_MS`. When that
/// gives more than `MAX_FRAMES` frames, it samples every `SLOW_KEYFRAME_MS` instead.
///
/// `None` when there is nothing to show: the window is under 2 s, the ring has no output in it, or
/// every frame is blank. A ring that starts too late is `Some` with `covered == false`.
#[allow(dead_code)] // `plan` calls `replay_every` itself; this is the one-step form the tests use
pub fn replay(snap: &Snapshot, start_ms: u64, end_ms: u64) -> Option<Replayed> {
    let fine = replay_every(snap, start_ms, end_ms, KEYFRAME_MS)?;
    if fine.frames.len() <= MAX_FRAMES {
        return Some(fine);
    }
    replay_every(snap, start_ms, end_ms, SLOW_KEYFRAME_MS)
}

/// `replay` with a chosen keyframe interval. The plan uses 200 ms when the JSON would be too big.
pub fn replay_every(snap: &Snapshot, start_ms: u64, end_ms: u64, every_ms: u32) -> Option<Replayed> {
    if every_ms == 0 || end_ms <= start_ms || end_ms - start_ms < MIN_WINDOW_MS {
        return None;
    }
    let start = start_ms as i64;
    let end = end_ms as i64;
    let lo = start - PAD_BEFORE_MS;
    let hi = settled_end(snap, end);

    // Events before the window are fed without sampling, so the screen is right when the turn
    // starts. Events in the window are kept for sampling. Time only moves forward.
    let mut parser = vt100::Parser::new(grid(snap.base_rows), grid(snap.base_cols), 0);
    let mut sampled: Vec<(&Event, i64)> = Vec::new();
    let mut last_t = lo;
    let mut any_output = false;
    for event in &snap.events {
        if !matches!(event.kind, Kind::Output | Kind::Resize) {
            continue;
        }
        let t = unix_ms(snap, event);
        if t < lo {
            apply(&mut parser, snap, event);
            continue;
        }
        if t > hi {
            continue;
        }
        let t = t.max(last_t);
        last_t = t;
        any_output |= event.kind == Kind::Output;
        sampled.push((event, t));
    }
    if !any_output {
        return None;
    }

    // Squeezed time: each gap counts at most GAP_MAX_MS. The tail after the last event counts too.
    let mut knots: Vec<(i64, u64)> = Vec::with_capacity(sampled.len() + 2);
    knots.push((lo, 0));
    let mut squeezed: u64 = 0;
    let mut prev = lo;
    for &(_, t) in &sampled {
        squeezed += ((t - prev) as u64).min(GAP_MAX_MS);
        prev = t;
        knots.push((t, squeezed));
    }
    squeezed += ((hi - prev) as u64).min(GAP_MAX_MS);
    knots.push((hi, squeezed));

    let total = squeezed as f64;
    let speed = (total / REPLAY_MAX_MS as f64).max(1.0);
    let length_ms = ((total / speed).round() as u32).min(REPLAY_MAX_MS);
    let time_map = TimeMap { knots, speed, length_ms };
    // Active time: the window after squeezing. The map is linear inside each gap, so the squeezed
    // span between the window's two ends is exactly the time that is left.
    let active_ms = time_map.squeezed_at(end) - time_map.squeezed_at(start);
    // The turn's own part of the clip: the settle tail after its end does not count in the speed.
    let clip = time_map.clip_at(end).saturating_sub(time_map.clip_at(start)).max(1) as f64;
    let speed_x = (active_ms / clip).round().max(1.0) as u32;
    let raw_speed_x = ((end - start) as f64 / length_ms.max(1) as f64).round().max(1.0) as u32;

    let stamps: Vec<u32> = sampled.iter().map(|&(_, t)| time_map.clip_at(t)).collect();
    let mut frames = Vec::new();
    let mut last_state: Option<Vec<u8>> = None;
    let mut next = 0;
    for tick in ticks(length_ms, every_ms) {
        while next < sampled.len() && stamps[next] <= tick {
            apply(&mut parser, snap, sampled[next].0);
            next += 1;
        }
        // The full screen state (text, attributes, cursor). A frame is kept only when it changed,
        // plus the last one, so the video holds the final screen to the end.
        let state = parser.screen().state_formatted();
        let is_last = tick == length_ms;
        if is_last || last_state.as_ref() != Some(&state) {
            frames.push(raw_frame(parser.screen(), tick));
            last_state = Some(state);
        }
    }
    if frames.iter().all(RawFrame::is_blank) {
        return None;
    }
    let (cols, rows) = pad(&mut frames);

    // The ring covers the turn from its own start (when it has not dropped anything) or from its
    // oldest kept event (when it has). Coverage below half of the turn is not usable.
    let ring_from = if snap.truncated {
        snap.events.first().map(|event| unix_ms(snap, event)).unwrap_or(end)
    } else {
        snap.start_unix_ms
    };
    let from = ring_from.max(start);
    let covered = (end - from) * 2 >= end - start;

    Some(Replayed {
        cols,
        rows,
        frames,
        speed_x,
        raw_speed_x,
        length_ms,
        covered,
        truncated: snap.truncated,
        time_map,
    })
}

/// The real time the replay stops at: the turn's end plus its padding, extended while output keeps
/// coming (the agent drawing its answer). The tail stops at a quiet gap of `SETTLE_QUIET_MS`, at the
/// first input after the turn (the person typing the next message), and at `SETTLE_MAX_MS`.
fn settled_end(snap: &Snapshot, end: i64) -> i64 {
    let mut hi = end + PAD_AFTER_MS;
    let limit = end + SETTLE_MAX_MS;
    let mut seen_until = end;
    for event in &snap.events {
        let t = unix_ms(snap, event);
        if t <= end {
            continue;
        }
        match event.kind {
            Kind::Input => {
                // The next message: what it echoes is not this turn's.
                hi = hi.min(t);
                break;
            }
            Kind::Output => {
                if t - seen_until > SETTLE_QUIET_MS || t > limit {
                    break;
                }
                seen_until = t;
                hi = hi.max((t + PAD_AFTER_MS).min(limit + PAD_AFTER_MS));
            }
            Kind::Resize | Kind::Marker => {}
        }
    }
    hi
}

/// Clip times of the keyframes: every `every` ms below the length, then the length itself.
fn ticks(length: u32, every: u32) -> impl Iterator<Item = u32> {
    (0..length).step_by(every as usize).chain(std::iter::once(length))
}

fn unix_ms(snap: &Snapshot, event: &Event) -> i64 {
    snap.start_unix_ms + (event.t_us / 1000) as i64
}

fn grid(n: u16) -> u16 {
    n.clamp(1, MAX_GRID)
}

/// Feeds one output event into the screen, or applies one resize event. Anything else is skipped.
fn apply(parser: &mut vt100::Parser, snap: &Snapshot, event: &Event) {
    match event.kind {
        Kind::Output => parser.process(snap.data(event)),
        Kind::Resize => {
            let cols = (event.aux >> 16) as u16;
            let rows = (event.aux & 0xffff) as u16;
            if cols > 0 && rows > 0 {
                parser.screen_mut().set_size(rows.min(MAX_GRID), cols.min(MAX_GRID));
            }
        }
        Kind::Input | Kind::Marker => {}
    }
}

fn raw_frame(screen: &vt100::Screen, at_ms: u32) -> RawFrame {
    let (rows, cols) = screen.size();
    let mut out_rows = Vec::with_capacity(rows as usize);
    let mut wrapped = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let cells = (0..cols)
            .map(|col| match screen.cell(row, col) {
                Some(cell) => RawCell { ch: cell_text(cell), style: style_of(cell) },
                None => blank_cell(),
            })
            .collect();
        out_rows.push(cells);
        wrapped.push(screen.row_wrapped(row));
    }
    let cursor = if screen.hide_cursor() { None } else { Some(screen.cursor_position()) };
    RawFrame { at_ms, rows: out_rows, cursor, wrapped }
}

fn cell_text(cell: &vt100::Cell) -> String {
    if cell.is_wide_continuation() {
        String::new()
    } else if cell.has_contents() {
        cell.contents().to_string()
    } else {
        " ".to_string()
    }
}

fn blank_cell() -> RawCell {
    RawCell { ch: " ".to_string(), style: 0 }
}

fn style_of(cell: &vt100::Cell) -> u8 {
    let mut style = 0;
    if cell.bold() {
        style |= BOLD;
    }
    if cell.dim() || is_dark(cell.fgcolor()) {
        style |= DIM;
    }
    if cell.inverse() {
        style |= INVERSE;
    }
    if cell.underline() {
        style |= UNDERLINE;
    }
    if cell.bgcolor() != vt100::Color::Default {
        style |= HIGHLIGHT;
    }
    style
}

/// A foreground colour under luminance 90 (of 255) reads as dim. The default colour does not.
fn is_dark(color: vt100::Color) -> bool {
    match rgb_of(color) {
        Some((r, g, b)) => (299 * r + 587 * g + 114 * b) / 1000 < 90,
        None => false,
    }
}

fn rgb_of(color: vt100::Color) -> Option<(u32, u32, u32)> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Rgb(r, g, b) => Some((r as u32, g as u32, b as u32)),
        vt100::Color::Idx(i) => Some(indexed_rgb(i)),
    }
}

/// The usual xterm colours: the 16 basic ones, the 6x6x6 cube, then the grey ramp.
fn indexed_rgb(i: u8) -> (u32, u32, u32) {
    const BASIC: [(u32, u32, u32); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    if i < 16 {
        return BASIC[i as usize];
    }
    if i < 232 {
        let n = i as u32 - 16;
        let level = |v: u32| if v == 0 { 0 } else { 55 + 40 * v };
        return (level(n / 36), level((n / 6) % 6), level(n % 6));
    }
    let grey = 8 + 10 * (i as u32 - 232);
    (grey, grey, grey)
}

/// Pads every frame to the largest grid, so each frame has the same rows and columns.
fn pad(frames: &mut [RawFrame]) -> (u16, u16) {
    let cols = frames.iter().flat_map(|f| f.rows.iter().map(Vec::len)).max().unwrap_or(0);
    let rows = frames.iter().map(|f| f.rows.len()).max().unwrap_or(0);
    for frame in frames.iter_mut() {
        while frame.rows.len() < rows {
            frame.rows.push(vec![blank_cell(); cols]);
            frame.wrapped.push(false);
        }
        for row in frame.rows.iter_mut() {
            row.resize(cols, blank_cell());
        }
    }
    (cols as u16, rows as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unix time of the ring's start, in ms.
    const START: i64 = 1_000_000;

    type Item = (u64, Kind, u32, Vec<u8>);

    /// A snapshot built by hand. Each item is (ms after the ring start, kind, aux, bytes).
    fn snap_with(base: (u16, u16), truncated: bool, items: Vec<Item>) -> Snapshot {
        let mut bytes = Vec::new();
        let mut events = Vec::new();
        for (ms, kind, aux, data) in items {
            let off = bytes.len() as u64;
            bytes.extend_from_slice(&data);
            events.push(Event { t_us: ms * 1000, off, len: data.len() as u32, aux, kind });
        }
        Snapshot {
            start_unix_ms: START,
            base_cols: base.0,
            base_rows: base.1,
            events,
            bytes,
            first_off: 0,
            truncated,
        }
    }

    fn out(ms: u64, text: &str) -> Item {
        (ms, Kind::Output, 0, text.as_bytes().to_vec())
    }

    fn resize(ms: u64, cols: u16, rows: u16) -> Item {
        (ms, Kind::Resize, (u32::from(cols) << 16) | u32::from(rows), Vec::new())
    }

    /// The window `[start, end]` in ms after the ring start, as unix ms.
    fn window(start: u64, end: u64) -> (u64, u64) {
        (START as u64 + start, START as u64 + end)
    }

    fn line(frame: &RawFrame, row: usize) -> String {
        frame.rows[row].iter().map(|c| c.ch.as_str()).collect::<String>().trim_end().to_string()
    }

    fn cell(frame: &RawFrame, row: usize, col: usize) -> &RawCell {
        &frame.rows[row][col]
    }

    fn last(replayed: &Replayed) -> &RawFrame {
        replayed.frames.last().expect("at least one frame")
    }

    #[test]
    fn plain_lines_are_drawn_in_the_window() {
        let snap = snap_with((20, 4), false, vec![out(1000, "hello\r\nworld")]);
        let (start, end) = window(0, 5000);
        let replayed = replay(&snap, start, end).expect("replays");
        assert!(replayed.covered);
        assert!(!replayed.truncated);
        // Output starts 1 s into the window: the 1.3 s lead-in is squeezed to 400 ms, and so is the tail.
        assert_eq!(replayed.length_ms, 800);
        let frame = last(&replayed);
        assert_eq!(line(frame, 0), "hello");
        assert_eq!(line(frame, 1), "world");
        assert_eq!(replayed.cols, 20);
        assert_eq!(replayed.rows, 4);
        assert_eq!(frame.at_ms, 800);
    }

    #[test]
    fn a_ten_minute_pause_is_squeezed_to_400_ms() {
        let snap = snap_with((20, 4), false, vec![out(0, "a"), out(600_000, "b")]);
        let (start, end) = window(0, 600_500);
        let replayed = replay(&snap, start, end).expect("replays");
        let map = &replayed.time_map;
        let a = map.clip_ms(START as u64);
        let b = map.clip_ms(START as u64 + 600_000);
        assert_eq!(b - a, 400);
        assert_eq!(replayed.length_ms, 1100);
        // Active time is about 567 ms (the 10 min pause is cut to 400 ms, the rest is real), so the
        // badge stays at its floor of 1. The raw ratio still counts the pause: 600 500 / 1 100 = 546.
        assert_eq!(replayed.speed_x, 1);
        assert_eq!(replayed.raw_speed_x, 546);
    }

    #[test]
    fn a_pause_does_not_inflate_the_sped_up_badge() {
        // Busy 10 s, then a 9 min pause, then busy 10 s. Active time is about 20 s of real output.
        let mut items: Vec<Item> = (0..=100).map(|i| out(i * 100, "x")).collect();
        items.extend((0..=100).map(|i| out(560_000 + i * 100, "y")));
        let snap = snap_with((80, 4), false, items);
        let (start, end) = window(0, 570_000);
        let replayed = replay(&snap, start, end).expect("replays");
        // Active: 10 s + 10 s + the 400 ms the pause keeps = about 20.4 s, in an 18 s clip: about 1.1x.
        // The raw ratio (570 s over 18 s, about 32) is what the old badge showed.
        assert_eq!(replayed.length_ms, 18_000);
        assert_eq!(replayed.speed_x, 1);
        assert!(replayed.raw_speed_x > replayed.speed_x);
    }

    #[test]
    fn a_five_second_turn_plays_at_real_speed() {
        let items: Vec<Item> = (0..=50).map(|i| out(i * 100, "x")).collect();
        let snap = snap_with((80, 4), false, items);
        let (start, end) = window(0, 5000);
        let replayed = replay(&snap, start, end).expect("replays");
        assert_eq!(replayed.speed_x, 1);
        assert_eq!(replayed.length_ms, 5700);
    }

    #[test]
    fn a_six_minute_turn_is_sped_up_about_twenty_times() {
        let items: Vec<Item> = (0..=1200).map(|i| out(i * 300, "x")).collect();
        let snap = snap_with((80, 4), false, items);
        let (start, end) = window(0, 360_000);
        let replayed = replay(&snap, start, end).expect("replays");
        assert_eq!(replayed.speed_x, 20);
        assert_eq!(replayed.raw_speed_x, 20);
        assert_eq!(replayed.length_ms, 18_000);
        assert!(replayed.frames.len() <= MAX_FRAMES);
    }

    #[test]
    fn a_long_busy_stream_stays_within_the_frame_cap() {
        // 30 000 single-character events, 10 ms apart. The letters change, so every keyframe changes
        // the screen, even once the screen is full.
        let items: Vec<Item> = (0..30_000u64)
            .map(|i| out(i * 10, &((b'a' + (i % 26) as u8) as char).to_string()))
            .collect();
        let snap = snap_with((80, 24), false, items);
        let (start, end) = window(0, 300_000);
        let replayed = replay(&snap, start, end).expect("replays");
        assert!(replayed.frames.len() <= MAX_FRAMES, "got {}", replayed.frames.len());
        assert_eq!(replayed.frames[1].at_ms, SLOW_KEYFRAME_MS);
        assert!(replayed.length_ms <= REPLAY_MAX_MS);
    }

    #[test]
    fn output_before_the_window_is_fed_but_not_sampled() {
        let snap = snap_with((20, 4), false, vec![out(0, "OLD\r\n"), out(20_000, "NEW")]);
        let (start, end) = window(20_000, 25_000);
        let replayed = replay(&snap, start, end).expect("replays");
        assert_eq!(line(&replayed.frames[0], 0), "OLD");
        assert_eq!(line(last(&replayed), 0), "OLD");
        assert_eq!(line(last(&replayed), 1), "NEW");
        assert_eq!(replayed.time_map.clip_ms(START as u64), 0);
        assert!(replayed.frames.iter().all(|f| f.at_ms <= replayed.length_ms));
    }

    #[test]
    fn a_resize_in_the_turn_changes_the_grid_and_pads_the_frames() {
        let digits = "0123456789012345678901234567890123456789";
        let snap = snap_with(
            (80, 24),
            false,
            vec![out(0, digits), resize(1000, 20, 24), out(2000, "\r\nZ")],
        );
        let (start, end) = window(0, 4000);
        let replayed = replay(&snap, start, end).expect("replays");
        assert_eq!(replayed.cols, 80);
        assert_eq!(replayed.rows, 24);
        assert!(replayed.frames.iter().all(|f| f.rows.len() == 24 && f.wrapped.len() == 24));
        assert!(replayed.frames.iter().all(|f| f.rows.iter().all(|r| r.len() == 80)));
        // Before the resize, column 25 holds a digit. After it the grid is 20 wide, so it is padding.
        let before = replayed.frames.iter().find(|f| line(f, 0).starts_with("0123")).expect("digits drawn");
        assert_eq!(cell(before, 0, 25).ch, "5");
        let after = last(&replayed);
        assert_eq!(cell(after, 0, 25).ch, " ");
        assert_eq!(line(after, 1), "Z");
    }

    #[test]
    fn a_repaint_after_a_recorded_resize_lands_on_the_new_grid() {
        // A TUI draws a right border with absolute positions. ConPTY repaints at the new width right
        // after a resize, so the recorded resize must come first and the replay must use its grid.
        let wide = "[2J[1;1HHEADER[1;30H|[3;1Hbody text[3;30H|";
        let narrow = "[2J[1;1HHEADER[1;16H|[3;1Hbody text[3;16H|";
        let snap = snap_with((30, 4), false, vec![out(0, wide), resize(1000, 16, 4), out(1005, narrow)]);
        let (start, end) = window(0, 4000);
        let replayed = replay(&snap, start, end).expect("replays");
        let before = replayed.frames.iter().find(|f| cell(f, 0, 29).ch == "|").expect("the wide screen is drawn");
        assert_eq!(cell(before, 2, 29).ch, "|");
        let after = last(&replayed);
        assert_eq!(line(after, 0), "HEADER         |");
        assert_eq!(line(after, 2), "body text      |");
        // Nothing of the wide screen is left, and the padded columns are blank.
        assert!((16..30).all(|c| cell(after, 0, c).ch == " " && cell(after, 2, c).ch == " "));
        assert_eq!(replayed.cols, 30);
    }

    #[test]
    fn a_grow_and_a_shrink_in_one_turn_each_use_their_own_grid() {
        let snap = snap_with(
            (20, 3),
            false,
            vec![
                out(0, "[2J[1;1Hone[1;20H|"),
                resize(800, 40, 3),
                out(805, "[2J[1;1Hone[1;40H|"),
                resize(1600, 12, 3),
                out(1605, "[2J[1;1Hone[1;12H|"),
            ],
        );
        let (start, end) = window(0, 4000);
        let replayed = replay(&snap, start, end).expect("replays");
        let borders: Vec<usize> = replayed
            .frames
            .iter()
            .filter_map(|f| (0..40).rev().find(|&c| cell(f, 0, c).ch == "|"))
            .collect();
        for width in [20usize, 40, 12] {
            assert!(borders.contains(&(width - 1)), "no frame has the border at column {width}: {borders:?}");
        }
        assert!(borders.iter().all(|c| [19, 39, 11].contains(c)), "a frame has a border on a wrong column: {borders:?}");
    }

    #[test]
    fn the_answer_drawn_just_after_the_turn_end_is_in_the_replay() {
        // The pane rests at 5.0 s (the summary's end) while the spinner is still on screen; the answer
        // is drawn at 5.9 s and 6.7 s, each within the quiet gap of the one before.
        let mut items: Vec<Item> = (0..=50).map(|i| out(i * 100, if i == 0 { "[2J[1;1Hworking" } else { "." })).collect();
        items.push(out(5_000, "[2J[1;1Hspinner Vibing"));
        items.push(out(5_900, "[2J[1;1Hthe answer"));
        items.push(out(6_700, "[2;1Hand its last line"));
        let snap = snap_with((40, 4), false, items);
        let (start, end) = window(0, 5_000);
        let replayed = replay(&snap, start, end).expect("replays");
        let last_frame = last(&replayed);
        assert_eq!(line(last_frame, 0), "the answer");
        assert_eq!(line(last_frame, 1), "and its last line");
    }

    #[test]
    fn the_tail_stops_at_a_quiet_gap_at_input_and_at_the_cap() {
        let base: Vec<Item> = (0..=50).map(|i| out(i * 100, if i == 0 { "[2J[1;1Hone" } else { "." })).collect();
        let (start, end) = window(0, 5_000);
        // A gap of 2 s after the end: the later output is another thing and is not replayed.
        let mut gap = base.clone();
        gap.push(out(7_000, "[2J[1;1Hlater"));
        let replayed = replay(&snap_with((40, 4), false, gap), start, end).expect("replays");
        assert!(line(last(&replayed), 0).starts_with("one"));
        // Typing after the turn ends the tail, even when output follows at once.
        let mut typed = base.clone();
        typed.push((5_300, Kind::Input, 0, b"next".to_vec()));
        typed.push(out(5_400, "[2J[1;1Hecho next"));
        let replayed = replay(&snap_with((40, 4), false, typed), start, end).expect("replays");
        assert!(line(last(&replayed), 0).starts_with("one"));
        // Output that never rests is cut at the cap (8 s) plus the padding.
        let mut busy = base;
        busy.extend((0..200).map(|i| out(5_100 + i * 100, ".")));
        let replayed = replay(&snap_with((40, 4), false, busy), start, end).expect("replays");
        // From 300 ms before the turn: 300 + 5 000 + the 8 000 cap + 700 padding at most; a stop at the end would be 6 000.
        assert!(replayed.length_ms <= 300 + 5_000 + 8_000 + 700 + 100, "{}", replayed.length_ms);
        assert!(replayed.length_ms > 10_000, "{}", replayed.length_ms);
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let snap = snap_with((20, 3), false, vec![out(0, "中文A\r\n")]);
        let (start, end) = window(0, 2000);
        let replayed = replay(&snap, start, end).expect("replays");
        let frame = last(&replayed);
        assert_eq!(cell(frame, 0, 0).ch, "中");
        assert_eq!(cell(frame, 0, 1).ch, "");
        assert_eq!(cell(frame, 0, 2).ch, "文");
        assert_eq!(cell(frame, 0, 3).ch, "");
        assert_eq!(cell(frame, 0, 4).ch, "A");
        assert_eq!(line(frame, 0), "中文A");
    }

    #[test]
    fn style_bits_follow_the_cell_attributes() {
        let text = "\x1b[1mB\x1b[0m\x1b[7mI\x1b[0m\x1b[4mU\x1b[0m\x1b[41mR\x1b[0m\x1b[2mD\x1b[0m\x1b[30mK\x1b[0mP";
        let snap = snap_with((20, 3), false, vec![out(0, text)]);
        let (start, end) = window(0, 2000);
        let replayed = replay(&snap, start, end).expect("replays");
        let frame = last(&replayed);
        let styles: Vec<u8> = (0..7).map(|c| cell(frame, 0, c).style).collect();
        assert_eq!(styles, vec![BOLD, INVERSE, UNDERLINE, HIGHLIGHT, DIM, DIM, 0]);
    }

    #[test]
    fn a_line_longer_than_the_screen_is_marked_as_wrapped() {
        // 16 characters on a 10-column screen: row 0 continues on row 1.
        let snap = snap_with((10, 3), false, vec![out(0, "abcdefghijklmnop")]);
        let (start, end) = window(0, 2000);
        let replayed = replay(&snap, start, end).expect("replays");
        let frame = last(&replayed);
        assert!(frame.wrapped[0]);
        assert!(!frame.wrapped[1]);
        assert_eq!(line(frame, 0), "abcdefghij");
        assert_eq!(line(frame, 1), "klmnop");
    }

    #[test]
    fn the_cursor_is_reported_unless_it_is_hidden() {
        let (start, end) = window(0, 2000);
        let shown = snap_with((20, 3), false, vec![out(0, "ab")]);
        let replayed = replay(&shown, start, end).expect("replays");
        assert_eq!(last(&replayed).cursor, Some((0, 2)));
        let hidden = snap_with((20, 3), false, vec![out(0, "ab\x1b[?25l")]);
        let replayed = replay(&hidden, start, end).expect("replays");
        assert_eq!(last(&replayed).cursor, None);
    }

    #[test]
    fn a_ring_that_starts_late_is_not_covered() {
        // Dropped older bytes: the ring now starts at 8 s of a 10 s turn.
        let (start, end) = window(0, 10_000);
        let late = snap_with((20, 3), true, vec![out(8000, "late")]);
        let replayed = replay(&late, start, end).expect("replays");
        assert!(!replayed.covered);
        assert!(replayed.truncated);
        // Half of the turn is enough.
        let half = snap_with((20, 3), true, vec![out(5000, "half")]);
        assert!(replay(&half, start, end).expect("replays").covered);
    }

    #[test]
    fn input_and_markers_are_never_replayed() {
        let snap = snap_with(
            (40, 3),
            false,
            vec![
                (100, Kind::Input, 0, b"sk-SECRET-TYPED".to_vec()),
                (200, Kind::Marker, 0, b"task text SECRET".to_vec()),
                out(1000, "ok"),
            ],
        );
        let (start, end) = window(0, 3000);
        let replayed = replay(&snap, start, end).expect("replays");
        let all: String = replayed
            .frames
            .iter()
            .flat_map(|f| f.rows.iter().flatten().map(|c| c.ch.clone()))
            .collect();
        assert!(!all.contains("SECRET"));
        assert!(all.contains("ok"));
    }

    #[test]
    fn nothing_usable_gives_none() {
        // A window under 2 s.
        let short = snap_with((20, 3), false, vec![out(0, "a")]);
        let (start, end) = window(0, 1500);
        assert!(replay(&short, start, end).is_none());
        // An empty or inverted window.
        assert!(replay(&short, 5, 5).is_none());
        // Only input in the window: no output to draw.
        let typed = snap_with((20, 3), false, vec![(100, Kind::Input, 0, b"x".to_vec())]);
        let (start, end) = window(0, 3000);
        assert!(replay(&typed, start, end).is_none());
        // Output that is only spaces is blank.
        let blank = snap_with((20, 3), false, vec![out(0, "   ")]);
        assert!(replay(&blank, start, end).is_none());
    }

    #[test]
    fn frames_are_kept_only_when_the_screen_changes() {
        let snap = snap_with((20, 3), false, vec![out(0, "a")]);
        let (start, end) = window(0, 10_000);
        let replayed = replay(&snap, start, end).expect("replays");
        // The empty first screen, the screen after the one character at 300 ms, and the last one.
        assert_eq!(replayed.frames.len(), 3);
        assert_eq!(replayed.frames[0].at_ms, 0);
        assert_eq!(replayed.frames[1].at_ms, 300);
        assert_eq!(replayed.frames[2].at_ms, 700);
    }

    #[test]
    fn the_time_map_clamps_outside_the_replay() {
        let snap = snap_with((20, 3), false, vec![out(0, "a"), out(5000, "b")]);
        let (start, end) = window(0, 5000);
        let replayed = replay(&snap, start, end).expect("replays");
        assert_eq!(replayed.time_map.clip_ms(0), 0);
        assert_eq!(replayed.time_map.clip_ms(u64::MAX), replayed.length_ms);
        assert_eq!(replayed.time_map.length_ms(), replayed.length_ms);
    }

    #[test]
    fn the_keyframe_ticks_are_every_interval_then_the_end() {
        assert_eq!(ticks(300, 100).collect::<Vec<_>>(), vec![0, 100, 200, 300]);
        assert_eq!(ticks(250, 100).collect::<Vec<_>>(), vec![0, 100, 200, 250]);
        assert_eq!(ticks(0, 100).collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn a_replay_is_refused_when_the_window_is_empty_or_inverted() {
        let snap = snap_with((20, 3), false, vec![out(0, "a")]);
        assert!(replay_every(&snap, 10, 10, 100).is_none());
        assert!(replay_every(&snap, 10, 5, 100).is_none());
        assert!(replay_every(&snap, 0, 5000, 0).is_none());
    }
}
