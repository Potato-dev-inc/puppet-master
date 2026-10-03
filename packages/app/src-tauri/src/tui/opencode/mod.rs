//! OpenCode: the terminal TUI (`opencode`) and the same binary driven over its HTTP API
//! (`opencode_native`). Everything TUI-facing lives in this folder:
//!
//! - `screen` — the interactive TUI's menus and selectors
//!
//! The HTTP API client (keys, quota, messages) is a protocol backend, not a screen-scraped TUI,
//! and stays in `crate::opencode`.

pub mod read_only;
mod screen;

use super::{Features, Kind, TuiProfile};

pub struct OpenCode;

impl TuiProfile for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
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

    fn features(&self) -> Features {
        Features {
            kind: Kind::NativeProtocol,
            read_only_spawned_pane: true,
            ..Features::CLI
        }
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        screen::looks_like_menu(text)
    }
}
