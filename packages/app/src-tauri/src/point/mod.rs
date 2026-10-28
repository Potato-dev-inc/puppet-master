//! Point and Fix: point at the thing that is wrong in a running app, say what should change, and
//! send that to any agent pane. Decisions (SPEC n32 section 3.1):
//! 1. The preview is its own window (`preview-<n>`) that loads the app's loopback URL directly.
//! 2. The picker script is injected only into `preview-*` windows, and only at the top frame of its own origin.
//! 3. The page cannot call Puppet Master. Picks come back because Rust pulls them (`eval_with_callback`).
//! 4. The screenshot is taken natively (WebView2 on Windows), never by the page.
//! 5. Typing happens in Puppet Master's own Point box window, never in the page.
//! 6. Sending goes only through the pane outbox. Nothing sends by itself.
//! 7. Screenshots live in the target pane's `.puppet-master/shots/` folder, newest 50 kept.
//! 8. A source file is named only when it resolves to a real file inside a pane folder.
//! 9. Point and Fix is on by default. It has no setting in v1.
//! This file holds the shared data types (plain structs, no logic). Each submodule adds its own logic.

// Scaffold: the types below are used by the phase 1 tasks, so the build does not warn until then.
#![allow(dead_code)]

pub mod box_window;
pub mod capture;
pub mod message;
pub mod origin;
pub mod pick;
pub mod session;
pub mod shot;
pub mod source;
pub mod target;
pub mod window;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// "Point another" adds up to this many elements to one message.
pub const MAX_PICKS: usize = 3;
/// A Point session (its picks and drafts) ends after this many milliseconds without activity.
pub const SESSION_TTL_MS: u64 = 10 * 60_000;
/// Caps for page-derived text, in characters (not bytes).
pub const MAX_TEXT: usize = 160;
pub const MAX_SELECTOR: usize = 300;
pub const MAX_HTML: usize = 300;
pub const MAX_TITLE: usize = 120;
/// Screenshots kept per project `.puppet-master/shots/` folder.
pub const KEEP_SHOTS: usize = 50;

/// A rectangle in CSS pixels, viewport coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The viewport size in CSS pixels and the device pixel ratio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub w: f64,
    pub h: f64,
    pub dpr: f64,
}

/// A file hint the page gives (dev builds only). `how` says where it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHint {
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub how: SourceHow,
}

/// Where a source hint came from. `Stack` gives a file only, never a line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceHow {
    DataAttr,
    ReactDebugSource,
    SvelteMeta,
    VueFile,
    Stack,
}

/// A source file that really exists inside a pane folder. `rel_path` uses `/` separators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub rel_path: String,
    pub line: Option<u32>,
}

/// The cleaned facts about one picked element (after caps and redaction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickFacts {
    /// Short page address, e.g. "localhost:5173/pricing".
    pub page: String,
    pub url: String,
    pub title: String,
    /// Tag name, e.g. "button".
    pub tag: String,
    /// Short label, e.g. "button.card-cta" or "button#buy".
    pub label: String,
    pub selector: String,
    pub text: String,
    pub html: String,
    pub rect: Rect,
    pub viewport: Viewport,
    pub attrs: Vec<(String, String)>,
    pub styles: Vec<(String, String)>,
    pub component: Option<String>,
    pub source_hint: Option<SourceHint>,
}

/// One picked element, kept in the session until it is sent or discarded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub id: String,
    pub preview_id: String,
    pub facts: PickFacts,
    pub source: Option<SourceRef>,
    pub shot: Option<Shot>,
    /// Why there is no screenshot. Rust-made text only, so it is not read back from JSON.
    #[serde(skip_deserializing, default)]
    pub no_shot_reason: Option<&'static str>,
    pub created_ms: u64,
}

/// A saved screenshot of one pick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub abs_path: PathBuf,
    /// Relative to the agent's folder, e.g. ".puppet-master/shots/pick-20261028-223104-1.png".
    pub rel_path: String,
    pub width: u32,
    pub height: u32,
}
