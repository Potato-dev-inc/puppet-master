//! OpenAI Codex CLI (`codex`). Everything Codex-specific lives in this folder:
//!
//! - `headless` — `codex exec` command line, session resume, JSONL output parsing
//!
//! Interactive-pane heuristics for Codex are not special yet and use the generic defaults; add a
//! `screen.rs` here when they need to be.

mod headless;

use super::{Features, HeadlessLaunch, LaunchSpec, TuiProfile};

pub struct Codex;

impl TuiProfile for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn features(&self) -> Features {
        Features {
            read_only_headless: true,
            resumable_headless: true,
            ..Features::CLI
        }
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
}
