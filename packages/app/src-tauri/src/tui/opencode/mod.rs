//! OpenCode: the terminal TUI (`opencode`) and the same binary driven over its HTTP API
//! (`opencode_native`). Everything TUI-facing lives in this folder:
//!
//! - `screen` — the interactive TUI's menus and selectors
//!
//! The HTTP API client (keys, quota, messages) is a protocol backend, not a screen-scraped TUI,
//! and stays in `crate::opencode`.

pub mod read_only;
mod screen;

use super::{Features, InstructionFile, Kind, StandbyScreen, StandbySupport, TuiProfile};

/// OpenCode reads `AGENTS.md` (falling back to `CLAUDE.md` only when it is missing): opencode.ai/docs/rules.
fn agents_md() -> Vec<InstructionFile> {
    vec![InstructionFile::new("AGENTS.md")]
}

pub struct OpenCode;

impl TuiProfile for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn instruction_files(&self) -> Vec<InstructionFile> {
        agents_md()
    }

    /// Visible panes only: the TUI takes its task typed into the pane, not on stdin.
    fn standby_support(&self) -> StandbySupport {
        StandbySupport {
            visible: true,
            headless: false,
            experimental: false,
        }
    }

    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        screen::standby_screen(screen)
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        screen::looks_like_menu(text)
    }
}

pub struct OpenCodeNative;

impl TuiProfile for OpenCodeNative {
    fn id(&self) -> &'static str {
        "opencode_native"
    }

    fn instruction_files(&self) -> Vec<InstructionFile> {
        agents_md()
    }

    fn features(&self) -> Features {
        Features {
            kind: Kind::NativeProtocol,
            read_only_spawned_pane: true,
            ..Features::CLI
        }
    }

    fn run_options(&self) -> super::RunOptionSupport {
        // Verified live: the native worker's bash tool runs without any permission opt-in.
        super::RunOptionSupport {
            allow_shell: true,
            shell_default_on: true,
            ..Default::default()
        }
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        screen::looks_like_menu(text)
    }
}
