//! Claude Code CLI (`claude`). Everything Claude-specific lives in this folder:
//!
//! - `headless` — command line for headless runs and reading their JSON output
//! - `screen` — whether the interactive TUI is working (drives how soon a pane reads idle)
//!
//! Other interactive-pane heuristics for Claude (status, approvals) use the generic defaults.

mod headless;
mod screen;

use super::{Features, HeadlessLaunch, LaunchSpec, TuiProfile};

pub struct Claude;

impl TuiProfile for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn features(&self) -> Features {
        Features {
            read_only_headless: true,
            ..Features::CLI
        }
    }

    fn is_installed(&self) -> bool {
        super::program_on_path("claude")
    }

    fn headless_command(&self, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        headless::command(launch)
    }

    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        headless::extract_result(output, exit_code)
    }

    fn extract_provider_session(&self, output: &str) -> Option<String> {
        headless::extract_provider_session(output)
    }

    fn extract_model(&self, output: &str) -> Option<String> {
        headless::extract_model(output)
    }

    fn busy_marker(&self, visible_tail: &str) -> Option<bool> {
        Some(screen::shows_work_in_progress(visible_tail))
    }

    /// No activity row and 800ms of stillness: the turn is over (live capture of a finished turn).
    fn quiet_idle_after(&self) -> std::time::Duration {
        std::time::Duration::from_millis(800)
    }

    fn is_chrome_line(&self, lower: &str) -> bool {
        screen::is_chrome_line(lower)
    }

    /// Typing the prompt and Enter in one burst is taken for a paste: Claude drops the Enter as an
    /// "invisible character" and waits for the user to "review and press Enter to send" (live,
    /// v2.1.287). The verified protocol types the text, confirms it is in the box, then sends Enter
    /// on its own. There is no clear sequence: Ctrl+U is not a kill-line key there, it is typed
    /// into the box as another invisible character.
    fn verifies_prompt_delivery(&self) -> bool {
        true
    }
}
