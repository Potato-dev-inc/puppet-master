//! Dot-matrix state glyph for the tray icon, in the visual language of the cockpit's `DotSpinner`
//! (`packages/app/src/components/cockpit/DotSpinner.tsx`): needs-you = plus with a bright centre,
//! running = a sweep round the perimeter, idle = dim corners, error = X.
//!
//! Everything here is pure data: aggregate counts -> pattern -> a 3x3 grid of dot alphas. The
//! rasteriser lives in `raster.rs`; the runtime thread in `tray_icon.rs`.

use std::time::Duration;

/// Alpha of a dot that is "off" (the cockpit draws off dots at 20%; a bit lower here reads
/// better at 16 px).
pub const OFF: f32 = 0.15;
pub const DIM: f32 = 0.55;
pub const ON: f32 = 1.0;

/// Number of frames in the running sweep (one per perimeter cell).
pub const SWEEP_FRAMES: u8 = 8;
/// About 4 frames per second. A full turn takes two seconds.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(250);

/// Dot alphas, `[row][col]`.
pub type Levels = [[f32; 3]; 3];

/// Perimeter cells `(row, col)` in clockwise order from the top-left corner.
pub const PERIMETER: [(usize, usize); 8] = [(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (2, 1), (2, 0), (1, 0)];

/// Tail of the sweep: alpha by distance behind the head.
const TAIL: [f32; 5] = [1.0, 0.7, 0.45, 0.28, 0.18];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    /// Plus shape, bright centre.
    NeedsYou,
    /// X.
    Error,
    /// Animated sweep, frame `0..SWEEP_FRAMES`.
    Running(u8),
    /// Running with animation switched off: a static perimeter ring.
    RunningStatic,
    /// Dim corners.
    Idle,
}

/// What the panes add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Aggregate {
    pub needs_you: u32,
    pub errors: u32,
    pub running: u32,
    /// The one attention count (`attention::counts().blocking`) when it is known. The badge and the
    /// tooltip show it, so the tray says what the Inbox, the phone and the digest say.
    pub blocking: Option<u32>,
}

impl Aggregate {
    /// Count pane statuses (`running | waiting_input | idle | error`). Anything else counts as idle.
    pub fn from_statuses<'a>(statuses: impl IntoIterator<Item = &'a str>) -> Self {
        let mut agg = Aggregate::default();
        for s in statuses {
            match s {
                "waiting_input" => agg.needs_you += 1,
                "error" => agg.errors += 1,
                "running" => agg.running += 1,
                _ => {}
            }
        }
        agg
    }

    /// The attention count is known: the badge and the tooltip use it instead of counting statuses.
    pub fn with_blocking(mut self, blocking: u32) -> Self {
        self.blocking = Some(blocking);
        self
    }

    /// The taskbar badge count: the attention count when known (questions, stuck panes and consent
    /// cards count too), else waiting_input plus error panes.
    pub fn attention(&self) -> u32 {
        self.blocking.unwrap_or(self.needs_you + self.errors)
    }
}

/// Priority: needs-you, then error, then running, then idle.
pub fn pattern_for(agg: &Aggregate, animate: bool, frame: u8) -> Pattern {
    if agg.needs_you > 0 {
        Pattern::NeedsYou
    } else if agg.errors > 0 {
        Pattern::Error
    } else if agg.running > 0 {
        if animate {
            Pattern::Running(frame % SWEEP_FRAMES)
        } else {
            Pattern::RunningStatic
        }
    } else {
        Pattern::Idle
    }
}

/// The 3x3 alpha grid of a pattern.
pub fn levels(pattern: Pattern) -> Levels {
    let mut g = [[OFF; 3]; 3];
    match pattern {
        Pattern::NeedsYou => {
            g[1][1] = ON;
            for (r, c) in [(0, 1), (1, 0), (1, 2), (2, 1)] {
                g[r][c] = DIM;
            }
        }
        Pattern::Error => {
            for i in 0..3 {
                g[i][i] = ON;
                g[i][2 - i] = ON;
            }
        }
        Pattern::Running(frame) => {
            let head = (frame % SWEEP_FRAMES) as usize;
            for (k, &(r, c)) in PERIMETER.iter().enumerate() {
                let behind = (head + PERIMETER.len() - k) % PERIMETER.len();
                g[r][c] = TAIL.get(behind).copied().unwrap_or(OFF);
            }
        }
        Pattern::RunningStatic => {
            for &(r, c) in &PERIMETER {
                g[r][c] = DIM;
            }
        }
        Pattern::Idle => {
            for (r, c) in [(0, 0), (0, 2), (2, 0), (2, 2)] {
                g[r][c] = DIM;
            }
        }
    }
    g
}

/// Does this pattern need a timer?
pub fn is_animated(pattern: Pattern) -> bool {
    matches!(pattern, Pattern::Running(_))
}

/// Next frame for a running sweep (wraps).
pub fn advance(frame: u8) -> u8 {
    (frame + 1) % SWEEP_FRAMES
}

/// How long the ticker may sleep before it must act again: the frame interval while the sweep
/// animates, `None` (sleep until a state change) when nothing animates.
pub fn tick_after(pattern: Pattern) -> Option<Duration> {
    is_animated(pattern).then_some(FRAME_INTERVAL)
}

/// Tray tooltip: "Puppet Master — 2 need you · 3 working".
pub fn tooltip(agg: &Aggregate) -> String {
    let mut parts: Vec<String> = Vec::new();
    match agg.blocking {
        // One number, the same one the Inbox shows; errors are part of it.
        Some(n) if n > 0 => parts.push(if n == 1 { "1 needs you".into() } else { format!("{n} need you") }),
        Some(_) => {}
        None => {
            if agg.needs_you > 0 {
                parts.push(if agg.needs_you == 1 { "1 needs you".into() } else { format!("{} need you", agg.needs_you) });
            }
            if agg.errors > 0 {
                parts.push(if agg.errors == 1 { "1 error".into() } else { format!("{} errors", agg.errors) });
            }
        }
    }
    if agg.running > 0 {
        parts.push(format!("{} working", agg.running));
    }
    if parts.is_empty() {
        "Puppet Master".to_string()
    } else {
        format!("Puppet Master \u{2014} {}", parts.join(" \u{b7} "))
    }
}

/// The animation state machine behind the tray icon. Pure: the runtime thread feeds it
/// aggregate updates and timer ticks, and draws `pattern()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticker {
    agg: Aggregate,
    animate: bool,
    frame: u8,
}

impl Ticker {
    pub fn new(animate: bool) -> Self {
        Self { agg: Aggregate::default(), animate, frame: 0 }
    }

    /// New aggregate or a changed "Animated tray icon" setting. A sweep that starts begins at frame 0.
    pub fn update(&mut self, agg: Aggregate, animate: bool) {
        let was_animated = is_animated(self.pattern());
        self.agg = agg;
        self.animate = animate;
        if !was_animated {
            self.frame = 0;
        }
    }

    /// The timer fired: step the sweep. No-op when nothing animates.
    pub fn tick(&mut self) {
        if is_animated(self.pattern()) {
            self.frame = advance(self.frame);
        }
    }

    pub fn pattern(&self) -> Pattern {
        pattern_for(&self.agg, self.animate, self.frame)
    }

    pub fn aggregate(&self) -> Aggregate {
        self.agg
    }

    /// Sleep this long before the next `tick`; `None` = no timer at all (idle CPU is zero).
    pub fn next_wait(&self) -> Option<Duration> {
        tick_after(self.pattern())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agg(needs_you: u32, errors: u32, running: u32) -> Aggregate {
        Aggregate { needs_you, errors, running, blocking: None }
    }

    #[test]
    fn statuses_are_counted_and_unknown_ones_are_idle() {
        let a = Aggregate::from_statuses(["running", "waiting_input", "idle", "error", "running", "exited"]);
        assert_eq!(a, agg(1, 1, 2));
        assert_eq!(a.attention(), 2);
    }

    #[test]
    fn needs_you_beats_error_beats_running_beats_idle() {
        assert_eq!(pattern_for(&agg(1, 1, 5), true, 3), Pattern::NeedsYou);
        assert_eq!(pattern_for(&agg(2, 0, 0), true, 0), Pattern::NeedsYou);
        assert_eq!(pattern_for(&agg(0, 1, 5), true, 3), Pattern::Error);
        assert_eq!(pattern_for(&agg(0, 0, 5), true, 3), Pattern::Running(3));
        assert_eq!(pattern_for(&agg(0, 0, 0), true, 3), Pattern::Idle);
    }

    #[test]
    fn animation_off_gives_a_static_ring_instead_of_a_sweep() {
        assert_eq!(pattern_for(&agg(0, 0, 2), false, 5), Pattern::RunningStatic);
        assert!(!is_animated(Pattern::RunningStatic));
    }

    #[test]
    fn pattern_shapes_match_the_cockpit_dot_spinner() {
        let on = |l: &Levels| -> Vec<(usize, usize)> {
            (0..3).flat_map(|r| (0..3).map(move |c| (r, c))).filter(|&(r, c)| l[r][c] >= DIM).collect()
        };
        // needs-you: plus shape, centre brightest.
        let n = levels(Pattern::NeedsYou);
        assert_eq!(on(&n), vec![(0, 1), (1, 0), (1, 1), (1, 2), (2, 1)]);
        assert_eq!(n[1][1], ON);
        assert!(n[0][1] < n[1][1]);
        // error: X.
        assert_eq!(on(&levels(Pattern::Error)), vec![(0, 0), (0, 2), (1, 1), (2, 0), (2, 2)]);
        // idle: corners.
        assert_eq!(on(&levels(Pattern::Idle)), vec![(0, 0), (0, 2), (2, 0), (2, 2)]);
        // running ring never lights the centre.
        for f in 0..SWEEP_FRAMES {
            assert_eq!(levels(Pattern::Running(f))[1][1], OFF);
        }
    }

    #[test]
    fn the_sweep_head_walks_clockwise_round_the_perimeter() {
        for f in 0..SWEEP_FRAMES {
            let l = levels(Pattern::Running(f));
            let (hr, hc) = PERIMETER[f as usize];
            assert_eq!(l[hr][hc], ON, "frame {f} head");
            // The cell just behind the head is the second brightest.
            let (br, bc) = PERIMETER[(f as usize + 7) % 8];
            assert!(l[br][bc] > l[hr][hc] * 0.5 && l[br][bc] < ON);
        }
    }

    #[test]
    fn frames_cycle_and_wrap_while_running() {
        let mut t = Ticker::new(true);
        t.update(agg(0, 0, 1), true);
        let seen: Vec<Pattern> = (0..SWEEP_FRAMES + 2)
            .map(|_| {
                let p = t.pattern();
                t.tick();
                p
            })
            .collect();
        let expect: Vec<Pattern> = (0..SWEEP_FRAMES + 2).map(|i| Pattern::Running(i % SWEEP_FRAMES)).collect();
        assert_eq!(seen, expect);
        assert_eq!(FRAME_INTERVAL, Duration::from_millis(250));
        assert_eq!(t.next_wait(), Some(FRAME_INTERVAL));
    }

    #[test]
    fn animation_stops_when_nothing_runs() {
        let mut t = Ticker::new(true);
        t.update(agg(0, 0, 3), true);
        assert!(t.next_wait().is_some());
        t.update(agg(0, 0, 0), true);
        assert_eq!(t.pattern(), Pattern::Idle);
        assert_eq!(t.next_wait(), None, "no timer while idle");
        let before = t.clone();
        t.tick();
        assert_eq!(t, before, "a stray tick is a no-op");
        // Needs-you and error are static too.
        t.update(agg(1, 0, 3), true);
        assert_eq!(t.next_wait(), None);
        t.update(agg(0, 1, 3), true);
        assert_eq!(t.next_wait(), None);
        // Animation switched off: no timer even though agents run.
        t.update(agg(0, 0, 3), false);
        assert_eq!(t.pattern(), Pattern::RunningStatic);
        assert_eq!(t.next_wait(), None);
    }

    #[test]
    fn a_new_sweep_starts_at_frame_zero_but_a_running_one_keeps_its_place() {
        let mut t = Ticker::new(true);
        t.update(agg(0, 0, 1), true);
        t.tick();
        t.tick();
        assert_eq!(t.pattern(), Pattern::Running(2));
        t.update(agg(0, 0, 4), true); // still running: more agents, same sweep
        assert_eq!(t.pattern(), Pattern::Running(2));
        t.update(agg(0, 0, 0), true);
        t.update(agg(0, 0, 1), true);
        assert_eq!(t.pattern(), Pattern::Running(0));
    }

    #[test]
    fn the_attention_count_replaces_the_status_count_in_the_badge_and_tooltip() {
        // Two waiting panes and an error make 3 by status, but a question, a stuck pane and a consent
        // card also need the person: the badge says what the Inbox says.
        let a = agg(2, 1, 1).with_blocking(6);
        assert_eq!(a.attention(), 6);
        assert_eq!(tooltip(&a), "Puppet Master \u{2014} 6 need you \u{b7} 1 working");
        assert_eq!(tooltip(&agg(0, 1, 0).with_blocking(1)), "Puppet Master \u{2014} 1 needs you");
        assert_eq!(tooltip(&agg(0, 0, 2).with_blocking(0)), "Puppet Master \u{2014} 2 working");
        // The glyph pattern still follows the pane statuses.
        assert_eq!(pattern_for(&a, true, 0), Pattern::NeedsYou);
    }

    #[test]
    fn tooltip_lists_needs_errors_and_working() {
        assert_eq!(tooltip(&agg(2, 0, 3)), "Puppet Master \u{2014} 2 need you \u{b7} 3 working");
        assert_eq!(tooltip(&agg(1, 0, 0)), "Puppet Master \u{2014} 1 needs you");
        assert_eq!(tooltip(&agg(0, 2, 1)), "Puppet Master \u{2014} 2 errors \u{b7} 1 working");
        assert_eq!(tooltip(&agg(0, 0, 0)), "Puppet Master");
    }
}
