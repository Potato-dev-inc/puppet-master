//! The interactive Codex CLI TUI: what its screen shows about whether a turn is running, a dialog is
//! up, or the composer is empty.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::tui::{ComposerState, StandbyScreen, StartupGate};

/// The questions Codex asks before it trusts a folder.
pub(super) const TRUST_PHRASES: [&str; 5] = [
    "do you trust the files in this folder",
    "do you trust the contents of this directory",
    "allow codex to work in this folder",
    // Codex 0.162+: "Trust this folder? ... 1. Trust and continue 2. Quit" (`fixtures/trust_folder_live.txt`).
    "trust this folder?",
    "continue only if you trust these files",
];

/// The first-run login choices.
const LOGIN_PHRASES: [&str; 2] = ["sign in with chatgpt", "provide your own api key"];

/// Trust and approval prompts Codex draws in the pane. A standby never answers one.
const DIALOG_PHRASES: [&str; 8] = [
    "do you trust the files in this folder",
    "do you trust the contents of this directory",
    "allow codex to work in this folder",
    // Codex 0.162+: "Trust this folder? ... 1. Trust and continue 2. Quit" (`fixtures/trust_folder_live.txt`).
    "trust this folder?",
    "continue only if you trust these files",
    "would you like to run the following command",
    "would you like to make the following edits",
    "yes, proceed",
];

/// Login and update screens the user has to deal with.
/// Blocking screens only. The "Update available!" banner Codex keeps above a ready composer is
/// not one (live capture `fixtures/standby_ready_live.txt`); the update menu shown first is
/// (`fixtures/update_prompt_live.txt`: "1. Update now ... 3. Skip until next version").
const ATTENTION_PHRASES: [&str; 5] = [
    "sign in with chatgpt",
    "provide your own api key",
    "update now",
    "skip until next version",
    "press enter to continue",
];

/// Placeholder text Codex shows in an empty composer. The composer keeps no separate state for
/// it, so a row matching one of these exactly is taken as empty; anything else is typed text.
/// Codex rotates its hints between releases: an unlisted hint reads as NotReady, never Ready.
const COMPOSER_PLACEHOLDERS: [&str; 5] = [
    "ask codex to do anything",
    "explain this codebase",
    "find and fix a bug in @filename",
    "implement {feature}",
    "write tests for @filename",
];

/// The text after the composer's `\u{203a}`: the last such glyph on the screen, up to the end of its
/// line. Submitted messages are drawn with the same glyph above it, and a multi-line composer
/// continues on unmarked rows below its first line.
///
/// The glyph is searched for anywhere in the text, not at the start of a line: Codex fills a row to
/// the last column, the emulator marks it wrapped, and `contents()` then glues it to the next row,
/// so on a pane with history the composer sits at the END of one long line that starts with an old
/// message (`fixtures/submitted_live.txt`). Reading the first row that starts with the glyph took that
/// old message for the composer.
fn composer_row(screen: &str) -> Option<&str> {
    let at = screen.rfind('\u{203a}')?;
    screen[at + '\u{203a}'.len_utf8()..].lines().next()
}

/// The screen text readiness reads (`TuiProfile::input_screen`): the emulator's text without the
/// dimmed placeholder Codex draws in an empty composer (`\u{203a} Ask Codex to do anything`, faint,
/// SGR 2; real capture `fixtures/composer_placeholder_dim_0_162_1_live.ansi`). Codex rotates its hints
/// between releases (`COMPOSER_PLACEHOLDERS` lists the ones seen); the faint style is what they share,
/// and typed text is drawn in the normal intensity. Only the composer row (the last row that starts
/// with the glyph) is looked at; faint text anywhere else stays.
pub(super) fn without_placeholder(screen: &vt100::Screen) -> String {
    let (_, cols) = screen.size();
    let rows: Vec<String> = screen.rows(0, cols).collect();
    let Some(composer) = rows.iter().rposition(|row| row.starts_with('\u{203a}')) else {
        return screen.contents();
    };
    crate::tui::faint::contents_without(screen, |row, col, cell| usize::from(row) == composer && col > 0 && cell.dim())
}

/// What the composer holds. A turn in flight (`esc to interrupt`) wins: Codex keeps the composer
/// visible under the status row, and text typed there is queued, not waiting to be submitted.
/// `Unknown` when no composer row is on screen (startup, a menu, a full-screen view).
pub(super) fn composer_state(screen: &str) -> ComposerState {
    // A startup menu marks its selected option with the composer's glyph; it is not an input box.
    if startup_gate(screen).is_some() {
        return ComposerState::Unknown;
    }
    if shows_work_in_progress(screen) {
        return ComposerState::Working;
    }
    match composer_row(screen) {
        None => ComposerState::Unknown,
        Some(rest) => {
            let rest = rest.trim().trim_end_matches('\u{258c}').trim().to_ascii_lowercase();
            if rest.is_empty() || COMPOSER_PLACEHOLDERS.contains(&rest.as_str()) {
                ComposerState::Empty
            } else {
                ComposerState::HasText
            }
        }
    }
}

/// Codex 0.162 draws its composer while it is still starting (the banner's second row reads
/// `loading` and the logo animates). Keys typed then show up seconds later or not at all (live, this
/// branch's `live_permit_codex_fresh_pane_is_delivered_under_a_permit`: the task typed on that
/// screen was not visible for 2 s). Real capture: `fixtures/loading_0_162_1_live.txt`.
pub(super) fn still_loading(screen: &str) -> bool {
    let rows: Vec<&str> = screen.lines().map(str::trim).filter(|row| !row.is_empty()).collect();
    rows.windows(2)
        .any(|pair| pair[0].starts_with(">_ OpenAI Codex") && pair[1].eq_ignore_ascii_case("loading"))
}

/// Tool-failure rows Codex 0.162 draws in a turn, in two layouts:
///
/// - a group header `Explored · 2 failed` and each failed call `Read README.md (exit -1)` (real
///   capture: `fixtures/result_tool_failed_live.txt`). A nonzero exit code, negative when the process
///   never started, ends such a row; the `+ Show details` hint can follow it.
/// - a lone failed call `• Failed (exit -1) Get-Content README.md -TotalCount 1`, the exit code FIRST
///   and the command after it, with the reason on the next row (`└ Failed to create unified exec
///   process`). Real capture 0.162.1: `fixtures/result_tool_failed_exit_first_live.txt`.
static TOOL_FAILED_HEADER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\W*[A-Z][a-z]+ \u{b7} [1-9]\d* failed\s*$").expect("static regex"));
static TOOL_FAILED_EXIT_FIRST: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\W*Failed \(exit (?:-\d+|[1-9]\d*)\)\s+\S").expect("static regex"));
static TOOL_FAILED_ROW: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\W*[A-Z][A-Za-z]+ .*\(exit (?:-\d+|[1-9]\d*)\)(?:\s+\+ Show details)?\s*$").expect("static regex")
});

/// True when a reply shows Codex reporting that one of its own tool calls failed.
pub(super) fn reports_tool_failure(reply: &str) -> bool {
    reply
        .lines()
        .map(str::trim)
        .any(|row| TOOL_FAILED_HEADER.is_match(row) || TOOL_FAILED_ROW.is_match(row) || TOOL_FAILED_EXIT_FIRST.is_match(row))
}

/// Ctrl+U sixteen times. Ctrl+U clears the line before the cursor and does nothing in an empty
/// composer; on a blank line it joins it to the one above, so an N-line draft needs about 2N
/// presses. Checked live on 0.159.3: one press cleared a single line, sixteen in one write cleared
/// a four-line draft and a "[Pasted Content N chars]" placeholder, and an empty composer stayed
/// empty. Ctrl+C is not used: on an empty composer it starts Codex's quit confirmation.
pub(super) const CLEAR_COMPOSER: &str = "\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}\u{15}";

/// Esc: the update menu's own footer says `esc skip`. It skips this launch's notice and nothing
/// else ("Update now" runs an installer, "Skip until next version" writes a setting).
const SKIP_KEY: &str = "\u{1b}";

/// Screens Codex shows before its composer takes input. The update MENU (options "Update now",
/// "Skip", "Skip until next version") is one; the boxed "Update available!" banner that stays
/// above a ready composer is not (`fixtures/standby_ready_live.txt`). Approvals of a command or an
/// edit are not startup screens either: they are prompts (`pane_prompt`).
pub(super) fn startup_gate(screen: &str) -> Option<StartupGate> {
    let lower = screen.to_ascii_lowercase();
    if lower.contains("update now") && lower.contains("skip until next version") {
        return Some(StartupGate {
            id: "update_available",
            summary: "Codex is offering an update",
            dismiss: Some(SKIP_KEY),
        });
    }
    // "Hooks need review" (live 0.162.1, `fixtures/hooks_need_review.txt`): shown at start when the
    // project's `.codex/hooks.json` has entries the user has not trusted yet, e.g. Puppet Master's
    // Hook Lane entries. Esc is the screen's own "esc skip": it continues WITHOUT trusting, for this
    // launch only (checked live: nothing is saved and the screen returns at the next start). It
    // never picks "Trust all": trusting stays the user's own step in `/hooks`.
    if lower.contains("hooks need review") && lower.contains("continue without trusting") {
        return Some(StartupGate {
            id: "hooks_review",
            summary: "Codex asks you to review new hooks",
            dismiss: Some(SKIP_KEY),
        });
    }
    if TRUST_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return Some(StartupGate {
            id: StartupGate::WORKSPACE_TRUST,
            summary: "Codex asks whether to trust this folder",
            dismiss: None,
        });
    }
    if LOGIN_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return Some(StartupGate {
            id: "login",
            summary: "Codex needs you to sign in",
            dismiss: None,
        });
    }
    if lower.contains("press enter to continue") {
        return Some(StartupGate {
            id: "onboarding",
            summary: "Codex is waiting for you to continue",
            dismiss: None,
        });
    }
    None
}

/// Rows Codex draws around a reply that are not part of it: the footer (`GPT-6-Luna medium ·
/// D:pm-demo · <thread title>`: model, reasoning effort, folder, title), the shortcuts hint and the
/// `Worked for 5s • 11:29 AM` separator under a finished turn, the update notice box
/// (`✨ Update available! 0.159.3 -> 0.162.1`, `See full release notes:`) and the
/// `>_ OpenAI Codex (v0.159.3)` banner above the first message (live captures of a visible run,
/// 0.159.3). `lower` is the trimmed, lowercased row.
///
/// The notice box and the banner can reach the reply as ONE row: the box rows fill the terminal's
/// full width, so the screen text joins the wrapped rows of the box's tail, the banner and the
/// folder without a line break (live: a Codex run's result began with
/// `│ See full release notes: ... ││ https://github.com/openai/codex/releases/latest ... │╰───╯
/// >_ OpenAI Codex (v0.159.3)   D:\pm-demo`).
pub(super) fn is_chrome_line(lower: &str) -> bool {
    static FOOTER: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^[\w.\-/]+ (minimal|low|medium|high|xhigh)(?: \u{b7} |$)").expect("codex footer regex")
    });
    static WORKED: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^worked for [0-9hms ]+(?: \u{2022} .*)?$").expect("codex worked-for regex"));
    // The banner `>_ OpenAI Codex (v0.159.3)`, wherever it sits in the row.
    static BANNER: Lazy<Regex> =
        Lazy::new(|| Regex::new(r">_ openai codex \(v[0-9][0-9a-z.\-]*\)").expect("codex banner regex"));
    // The notice box's rows: box glyphs and the sparkle, then the notice's own text.
    static NOTICE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"^(?:[\u{2500}-\u{257f}\u{2728}\s]*(?:update available!|see full release notes)|[\u{2500}-\u{257f}]+\s*https://github\.com/openai/codex/releases)",
        )
        .expect("codex update notice regex")
    });
    lower.contains("? for shortcuts")
        || FOOTER.is_match(lower)
        || WORKED.is_match(lower)
        || BANNER.is_match(lower)
        || NOTICE.is_match(lower)
}

/// Whether the screen shows Codex working: the status row `Working (12s \u{2022} esc to interrupt)`.
pub(super) fn shows_work_in_progress(screen: &str) -> bool {
    screen.to_ascii_lowercase().contains("esc to interrupt")
}

/// Whether a pre-launched Codex pane is at an empty composer with no dialog or login screen on top.
/// Anything unrecognised is NotReady.
pub(super) fn standby_screen(screen: &str) -> StandbyScreen {
    let lower = screen.to_ascii_lowercase();
    if DIALOG_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::Dialog;
    }
    if ATTENTION_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return StandbyScreen::NeedsAttention;
    }
    if shows_work_in_progress(screen) {
        return StandbyScreen::NotReady;
    }
    // The composer is the last `\u{203a}` row: submitted messages are drawn with the same glyph above it.
    let composer = composer_row(screen);
    let empty = composer.is_some_and(|rest| {
        // The block cursor may sit in an empty composer.
        let rest = rest.trim().trim_end_matches('\u{258c}').trim().to_ascii_lowercase();
        rest.is_empty() || COMPOSER_PLACEHOLDERS.contains(&rest.as_str())
    });
    if empty && lower.contains("codex") {
        StandbyScreen::Ready
    } else {
        StandbyScreen::NotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-written from the Codex CLI's layout, not a live capture: banner, empty composer with a
    // placeholder hint, and the footer row.
    const READY: &str = ">_ OpenAI Codex (v0.1.0)\n\n  model: gpt-5-codex   directory: ~/project\n\n\u{203a} Ask Codex to do anything\n\n  ? for shortcuts\n";
    const READY_CURSOR: &str = ">_ OpenAI Codex (v0.1.0)\n\n\u{203a} \u{258c}\n\n  ? for shortcuts\n";
    // Hand-written: a turn in flight shows the status row above the composer.
    const WORKING: &str = ">_ OpenAI Codex (v0.1.0)\n\n\u{2022} Working (12s \u{2022} esc to interrupt)\n\n\u{203a} Ask Codex to do anything\n";
    // Hand-written: the trust dialog on first launch in a folder.
    const TRUST_DIALOG: &str = "> You are running Codex in ~/sandbox\n\n  Do you trust the files in this folder?\n\n\u{203a} 1. Yes, continue\n  2. No, quit\n\n  Press enter to continue\n";
    // Hand-written: an approval request for a shell command.
    const APPROVAL: &str = "  Would you like to run the following command?\n\n    $ cargo test\n\n\u{203a} 1. Yes, proceed (y)\n  2. No, and tell Codex what to do differently (esc)\n";
    // Hand-written: the login screen of a fresh install.
    const LOGIN: &str = ">_ Welcome to Codex\n\n  1. Sign in with ChatGPT\n  2. Provide your own API key\n";
    // Hand-written: the update notice shown above the composer.
    const UPDATE: &str = "Update available! 0.1.0 -> 0.2.0\nRun npm install -g @openai/codex to update.\n\u{203a} Ask Codex to do anything\n";
    // Hand-written: the text typed into the composer is not a placeholder.
    const TYPED: &str = ">_ OpenAI Codex (v0.1.0)\n\n\u{203a} fix the failing test in parser.rs\n";
    // Hand-written: the startup splash before the composer is drawn.
    const SPLASH: &str = ">_ OpenAI Codex (v0.1.0)\n\n  loading session...\n";

    #[test]
    fn a_live_capture_of_the_empty_composer_under_the_update_banner_is_ready() {
        // Real Codex 0.159 pane after skipping the update menu: banner on top, empty composer.
        let live = include_str!("fixtures/standby_ready_live.txt");
        assert_eq!(standby_screen(live), StandbyScreen::Ready);
    }

    #[test]
    fn a_live_capture_of_the_update_prompt_needs_attention() {
        // Captured from a real Codex 0.159 pane right after start: the update menu comes first.
        let live = include_str!("fixtures/update_prompt_live.txt");
        assert_eq!(standby_screen(live), StandbyScreen::NeedsAttention);
    }

    #[test]
    fn standby_screen_is_ready_only_at_an_empty_composer() {
        assert_eq!(standby_screen(READY), StandbyScreen::Ready);
        assert_eq!(standby_screen(READY_CURSOR), StandbyScreen::Ready, "a block cursor in an empty box");
        assert_eq!(standby_screen(TYPED), StandbyScreen::NotReady, "typed text in the box");
        assert_eq!(standby_screen(WORKING), StandbyScreen::NotReady, "a turn in progress");
        assert_eq!(standby_screen(SPLASH), StandbyScreen::NotReady, "startup splash");
        assert_eq!(standby_screen(""), StandbyScreen::NotReady);
    }

    #[test]
    fn dialogs_and_login_screens_are_not_ready() {
        assert_eq!(standby_screen(TRUST_DIALOG), StandbyScreen::Dialog);
        assert_eq!(standby_screen(APPROVAL), StandbyScreen::Dialog);
        assert_eq!(standby_screen(LOGIN), StandbyScreen::NeedsAttention);
        // The update banner above an empty composer is not blocking (live capture agrees).
        assert_eq!(standby_screen(UPDATE), StandbyScreen::Ready);
    }

    // Real captures from a live Codex 0.159.3 pane in D:/pm-demo (Windows ConPTY, 120 columns).
    // The panes had history, so rows that reach the last column are glued to the next one by the
    // emulator: the composer is at the END of one long line (see `composer_row`).
    const LIVE_UNSENT: &str = include_str!("fixtures/composer_unsent_live.txt");
    const LIVE_UNSENT_GLUED: &str = include_str!("fixtures/composer_unsent_glued_live.txt");
    const LIVE_SUBMITTED: &str = include_str!("fixtures/submitted_live.txt");
    const LIVE_WORKING: &str = include_str!("fixtures/working_live.txt");
    const LIVE_PASTED: &str = include_str!("fixtures/composer_pasted_live.txt");
    const LIVE_UPDATE_MENU: &str = include_str!("fixtures/update_prompt_live.txt");
    const LIVE_READY_BANNER: &str = include_str!("fixtures/standby_ready_live.txt");
    const LIVE_TRUST_FOLDER: &str = include_str!("fixtures/trust_folder_live.txt");

    #[test]
    fn live_composer_with_text_that_enter_did_not_submit_is_has_text() {
        // Bug #3: the task was typed, the Enter became a line break, the pane sat idle.
        assert_eq!(composer_state(LIVE_UNSENT), ComposerState::HasText);
        assert_eq!(composer_state(LIVE_UNSENT_GLUED), ComposerState::HasText);
        assert_eq!(composer_state(LIVE_PASTED), ComposerState::HasText, "a [Pasted Content N chars] placeholder is text");
    }

    #[test]
    fn live_screens_after_a_submit_read_as_a_started_turn() {
        assert_eq!(composer_state(LIVE_WORKING), ComposerState::Working);
        // The reply is in, the composer is empty again. The old message at the START of the glued
        // line begins with the composer glyph and must not be taken for the composer.
        assert_eq!(composer_state(LIVE_SUBMITTED), ComposerState::Empty);
        assert_eq!(composer_state(LIVE_READY_BANNER), ComposerState::Empty);
    }

    #[test]
    fn a_startup_menu_is_not_a_composer() {
        assert_eq!(composer_state(LIVE_UPDATE_MENU), ComposerState::Unknown);
        assert_eq!(composer_state(""), ComposerState::Unknown);
    }

    #[test]
    fn the_standby_check_reads_the_composer_on_a_glued_screen_too() {
        // Before: the first row starting with the glyph was the old message, so a pane with
        // history never read as Ready.
        assert_eq!(standby_screen(LIVE_SUBMITTED), StandbyScreen::Ready);
        assert_eq!(standby_screen(LIVE_UNSENT_GLUED), StandbyScreen::NotReady);
    }

    #[test]
    fn live_hooks_review_screen_is_skipped_with_esc_never_trusted() {
        for screen in [
            include_str!("fixtures/hooks_need_review.txt"),
            include_str!("fixtures/hooks_need_review_one_changed.txt"),
        ] {
            let gate = startup_gate(screen).expect("the hooks review screen is a startup gate");
            assert_eq!(gate.id, "hooks_review");
            // Esc = "esc skip" = continue without trusting. Enter would open the review and "2"
            // would trust every hook: neither is ever sent for the user.
            assert_eq!(gate.dismiss, Some("\u{1b}"));
        }
    }

    #[test]
    fn live_update_menu_is_a_dismissible_startup_gate() {
        // Bug #4: this screen was taken for a run's answer.
        let gate = startup_gate(LIVE_UPDATE_MENU).expect("the update menu is a startup gate");
        assert_eq!(gate.id, "update_available");
        // Esc is the menu's own "esc skip": never Enter (that runs "Update now") and never the
        // "Skip until next version" row (that writes a setting).
        assert_eq!(gate.dismiss, Some("\u{1b}"));
    }

    #[test]
    fn the_banner_above_a_ready_composer_and_ordinary_screens_are_not_gates() {
        for screen in [
            LIVE_READY_BANNER, LIVE_UNSENT, LIVE_UNSENT_GLUED, LIVE_SUBMITTED, LIVE_WORKING, LIVE_PASTED,
            READY, WORKING, UPDATE, TYPED, SPLASH,
        ] {
            assert_eq!(startup_gate(screen), None, "{screen:.80}");
        }
    }

    #[test]
    fn trust_login_and_onboarding_screens_are_gates_nobody_dismisses_for_the_user() {
        for (screen, id) in [
            (TRUST_DIALOG, "workspace_trust"),
            (LIVE_TRUST_FOLDER, "workspace_trust"),
            (LOGIN, "login"),
            ("Welcome to Codex\n\n  Press Enter to continue\n", "onboarding"),
        ] {
            let gate = startup_gate(screen).unwrap_or_else(|| panic!("{screen}"));
            assert_eq!(gate.id, id);
            assert_eq!(gate.dismiss, None, "{id} needs the user");
        }
        // The live 0.162 wording ("Trust this folder? ... 1. Trust and continue") has the composer glyph
        // on its selected row, so it must be a gate and a standby dialog, never read as a composer.
        assert_eq!(standby_screen(LIVE_TRUST_FOLDER), StandbyScreen::Dialog);
        assert!(crate::pane_prompt::dispatch_blocker("codex", LIVE_TRUST_FOLDER).is_some());
        // A command approval is a prompt (`pane_prompt`), not a startup screen.
        assert_eq!(startup_gate(APPROVAL), None);
    }

    /// Live v0.2.0: Codex's folder-trust screen was classified `idle`, so no Needs-you card (and none of
    /// its buttons) was ever raised. A screen only the person can pass is a waiting pane.
    #[test]
    fn a_startup_screen_only_the_person_can_pass_is_a_waiting_pane_and_nothing_else_is() {
        use crate::pty::status::PaneStatus;
        use crate::tui::TuiProfile;
        let codex = crate::tui::codex::Codex;
        let tail = |screen: &str| {
            let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
            lines[lines.len().saturating_sub(12)..].join("
")
        };
        for gate in [LIVE_TRUST_FOLDER, TRUST_DIALOG, LOGIN] {
            assert_eq!(codex.classify_screen(&tail(gate)), Some(PaneStatus::WaitingInput), "{gate:.60}");
            assert_eq!(crate::pty::status::classify_agent_observation("codex", gate, ""), PaneStatus::WaitingInput, "{gate:.60}");
        }
        for other in [LIVE_READY_BANNER, LIVE_UNSENT, LIVE_SUBMITTED, LIVE_WORKING, READY, WORKING, TYPED, UPDATE] {
            assert_eq!(codex.classify_screen(&tail(other)), None, "{other:.60}");
        }
    }

    #[test]
    fn the_clear_sequence_is_only_ctrl_u() {
        assert!(CLEAR_COMPOSER.chars().all(|ch| ch == '\u{15}'));
        assert!(CLEAR_COMPOSER.len() >= 8, "a multi-line draft needs about two presses per line");
    }

    #[test]
    fn the_profile_says_idle_for_every_live_idle_screen_and_busy_for_the_working_one() {
        use crate::tui::TuiProfile;
        let codex = crate::tui::codex::Codex;
        // The 12-row tail the classifier hands the profile.
        let tail = |screen: &str| {
            let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
            lines[lines.len().saturating_sub(12)..].join("\n")
        };
        for idle in [LIVE_READY_BANNER, LIVE_UNSENT, LIVE_SUBMITTED, LIVE_PASTED, READY, TYPED] {
            assert_eq!(codex.busy_marker(&tail(idle)), Some(false), "{idle:.60}");
        }
        assert_eq!(codex.busy_marker(&tail(LIVE_WORKING)), Some(true));
        assert_eq!(codex.busy_marker(&tail(WORKING)), Some(true));
    }

    #[test]
    fn the_footer_and_separator_rows_of_a_live_run_are_chrome_and_replies_are_not() {
        for row in ["gpt-6-luna medium · d:\\pm-demo · summarize readme.md", "gpt-5-codex high · ~/proj", "? for shortcuts", "worked for 5s • 11:29 am", "worked for 1m 3s"] {
            assert!(is_chrome_line(row), "{row}");
        }
        for row in [
            "• readme.md describes a tiny node.js demo for puppet master, where sum.js adds numbers.",
            "the release notes for codex are at https://github.com/openai/codex/releases/latest",
            "update available means a newer build exists",
            "the banner reads openai codex",
            "medium · rare",
            "high availability needs two nodes",
            "the worked for example is below",
            "• explored",
        ] {
            assert!(!is_chrome_line(row), "{row}");
        }
    }

    #[test]
    fn the_status_row_is_work_and_an_idle_composer_is_not() {
        assert!(shows_work_in_progress(WORKING));
        assert!(!shows_work_in_progress(READY));
    }

    #[test]
    fn a_live_capture_of_a_turn_whose_own_tool_failed_reports_a_tool_failure() {
        let live = include_str!("fixtures/result_tool_failed_live.txt");
        assert!(reports_tool_failure(live));
    }

    /// Codex 0.162.1 on this PC (VERIFY-R11 2c): `• Failed (exit -1) <command>`, the exit code first.
    #[test]
    fn a_live_capture_of_the_exit_first_failed_row_reports_a_tool_failure() {
        let live = include_str!("fixtures/result_tool_failed_exit_first_live.txt");
        assert!(reports_tool_failure(live));
        assert!(reports_tool_failure("• Failed (exit -1) Get-Content README.md -TotalCount 1"));
        assert!(reports_tool_failure("  Failed (exit 2) npm test"));
        // The same screen with the failed rows taken out is a normal turn.
        let without: String = live.lines().filter(|row| !row.contains("Failed")).collect::<Vec<_>>().join("\n");
        assert!(!reports_tool_failure(&without));
    }

    /// An empty composer shows the dimmed placeholder (`› Ask Codex to do anything`): that is an
    /// empty box, also on the screen of a finished turn that ends in tool failures.
    #[test]
    fn the_ask_codex_placeholder_is_an_empty_composer_on_the_live_failed_turn_screen() {
        let live = include_str!("fixtures/result_tool_failed_exit_first_live.txt");
        assert_eq!(composer_state(live), ComposerState::Empty);
        assert_eq!(composer_state(include_str!("fixtures/result_tool_failed_live.txt")), ComposerState::Empty);
    }

    #[test]
    fn ordinary_replies_and_prose_about_exit_codes_do_not_report_a_tool_failure() {
        for screen in [LIVE_SUBMITTED, LIVE_WORKING, LIVE_READY_BANNER, READY, WORKING] {
            assert!(!reports_tool_failure(screen), "{screen:.60}");
        }
        for reply in [
            "\u{2022} The script exits with (exit 1) when the file is missing.",
            "Explored \u{b7} 2 files",
            "\u{2022} Explored",
            "Explored \u{b7} 0 failed",
            "Ran npm test (exit 0)",
            "• Failed (exit 0) is what a passing run never prints",
            "• Failed to read README.md (exit code unknown)",
        ] {
            assert!(!reports_tool_failure(reply), "{reply}");
        }
    }
    // ---- the dimmed placeholder in the composer (VERIFY-R11: make sure it counts as empty) ----

    /// A fresh Codex 0.162.1 pane, 120x30, as the emulator is fed it (`/panes/{id}/screen`): the
    /// composer row is `\u{203a}` and the faint (SGR 2) hint `Ask Codex to do anything`.
    const PLACEHOLDER_ANSI: &str = include_str!("fixtures/composer_placeholder_dim_0_162_1_live.ansi");

    /// The screen text with (`true`) or without (`false`) the profile's attribute-aware reading.
    fn vt100_text(bytes: &str, as_the_app_does: bool) -> String {
        let mut parser = vt100::Parser::new(30, 120, 0);
        parser.process(bytes.as_bytes());
        if as_the_app_does {
            without_placeholder(parser.screen())
        } else {
            parser.screen().contents()
        }
    }

    fn read_as_the_app_does(bytes: &str) -> String {
        vt100_text(bytes, true)
    }

    #[test]
    fn the_dimmed_ask_codex_placeholder_of_a_live_pane_is_an_empty_composer() {
        let text = read_as_the_app_does(PLACEHOLDER_ANSI);
        assert!(!text.contains("Ask Codex"), "{text}");
        assert!(text.contains("OpenAI Codex") && text.contains("? for shortcuts"), "the rest of the screen stays: {text}");
        assert_eq!(composer_state(&text), ComposerState::Empty);
        // The plain text, with no attributes to go by, is still read from the known hints.
        assert_eq!(composer_state("\u{203a} Ask Codex to do anything\n  GPT-6-Luna medium"), ComposerState::Empty);
    }

    /// Codex rotates its hints between releases; the faint style is what they share. The live capture
    /// with its hint swapped for one `COMPOSER_PLACEHOLDERS` has never seen is still an empty box, and
    /// the same words drawn in the normal intensity (typed by the user) are still a draft.
    #[test]
    fn an_unlisted_faint_hint_is_empty_and_the_same_words_typed_are_a_draft() {
        let hint = "Summarize recent commits";
        let faint = PLACEHOLDER_ANSI.replace("Ask Codex to do anything", hint);
        assert!(faint.contains(hint), "the swap found the live hint");
        let plain = vt100_text(&faint, false);
        assert_eq!(composer_state(&plain), ComposerState::HasText, "text alone cannot tell: {plain}");
        assert_eq!(composer_state(&read_as_the_app_does(&faint)), ComposerState::Empty);
        let typed = faint.replace("\u{1b}[2mSummarize", "\u{1b}[22mSummarize");
        assert_eq!(composer_state(&read_as_the_app_does(&typed)), ComposerState::HasText);
    }
}
