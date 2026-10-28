//! The pick as the picker script sends it, and the cleaned facts (PF-2). Page input is untrusted.
use super::{Rect, SourceHint, Viewport};
use serde::Deserialize;

/// What the picker script sends (untrusted; capped and cleaned by `sanitize` in PF-2).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickInput {
    /// location.href (the fragment is dropped by `sanitize`).
    pub url: String,
    /// document.title.
    pub title: String,
    /// Tag name, e.g. "button".
    pub tag: String,
    /// Unique CSS selector made by the picker.
    pub selector: String,
    /// innerText (never input values).
    pub text: String,
    /// Opening tag only, no value attributes.
    pub html: String,
    /// CSS px, viewport coordinates.
    pub rect: Rect,
    pub viewport: Viewport,
    /// id, data-testid, aria-label, role, name, href (path only). Max 8 after `sanitize`.
    pub attrs: Vec<(String, String)>,
    /// Computed styles: font-size, font-weight, color, background-color, padding, margin, display, border-radius.
    pub styles: Vec<(String, String)>,
    /// Nearest framework component name, when the page shows one.
    pub component: Option<String>,
    /// File hint from the page, when the page shows one.
    pub source: Option<SourceHint>,
}
