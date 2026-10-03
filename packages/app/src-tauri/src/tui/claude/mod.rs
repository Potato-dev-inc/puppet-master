//! Claude Code CLI (`claude`). Everything Claude-specific lives in this folder:
//!
//! - `headless` — command line for headless runs and reading their JSON output
//!
//! Interactive-pane heuristics for Claude (status, approvals) are not special yet and use the
//! generic defaults; add a `screen.rs` here when they need to be.

mod headless;

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
