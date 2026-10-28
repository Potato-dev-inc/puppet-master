//! The pick as the picker script sends it, and the cleaned facts (PF-2). Page input is untrusted.
//!
//! `sanitize` is the only way from a `PickInput` to `PickFacts`. Page text is redacted first and cut
//! second (a cut must not leave half a secret behind). Only allow-listed attributes and styles are kept,
//! form values never are, and the rectangle is clamped to the viewport.
use super::origin::PreviewOrigin;
use super::{PickFacts, Rect, SourceHint, SourceHow, Viewport, MAX_HTML, MAX_SELECTOR, MAX_TEXT, MAX_TITLE};
use crate::redact;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

/// Longest page address or URL kept, in characters.
pub const MAX_URL: usize = 500;
/// Most attributes kept from one element.
pub const MAX_ATTRS: usize = 8;
/// Longest value kept for one style.
const MAX_STYLE: usize = 120;
/// Longest tag name kept, in characters.
const MAX_TAG: usize = 24;
/// Longest class name or id kept, in characters.
const MAX_NAME: usize = 40;
/// Longest source file hint kept, in characters.
const MAX_FILE: usize = 300;
/// Largest viewport side accepted, in CSS px.
const MAX_VIEWPORT_PX: f64 = 16_384.0;

/// The attributes the message may show. Anything else the page sends is dropped.
const ATTR_KEYS: [&str; 6] = ["id", "data-testid", "aria-label", "role", "name", "href"];
/// The computed styles the message may show.
const STYLE_KEYS: [&str; 8] = [
    "font-size",
    "font-weight",
    "color",
    "background-color",
    "padding",
    "margin",
    "display",
    "border-radius",
];

/// What the picker script sends (untrusted; capped and cleaned by `sanitize`).
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

/// Why a pick is refused. The text is for the person; it never repeats the page's own text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickRefusal {
    /// The page is not the preview's own origin.
    OtherOrigin,
    /// The URL, tag or viewport cannot be read.
    BadInput,
}

impl PickRefusal {
    pub fn message(&self) -> &'static str {
        match self {
            PickRefusal::OtherOrigin => "That pick is not from the page in this preview.",
            PickRefusal::BadInput => "That pick could not be read. Pick again.",
        }
    }
}

/// Clean one pick from the page: check the origin, redact, cut and clamp. Pure.
pub fn sanitize(input: PickInput, origin: &PreviewOrigin) -> Result<PickFacts, PickRefusal> {
    let parsed = tauri::Url::parse(&input.url).map_err(|_| PickRefusal::BadInput)?;
    if !origin.matches(&input.url) {
        return Err(PickRefusal::OtherOrigin);
    }
    let viewport = clean_viewport(&input.viewport).ok_or(PickRefusal::BadInput)?;
    let tag = clean_tag(&input.tag).ok_or(PickRefusal::BadInput)?;
    let rect = clamp_rect(&input.rect, &viewport);

    // The address: fragment dropped, then redacted (a token can sit in the query), then cut.
    let mut without_fragment = parsed.clone();
    without_fragment.set_fragment(None);
    let url = cut(&redact::text(without_fragment.as_str()).into_inner(), MAX_URL);
    let page = cut(&redact::text(&format!("{}:{}{}", origin.host, origin.port, parsed.path())).into_inner(), MAX_URL);

    let title = clean_text(&input.title, MAX_TITLE);
    let text = clean_text(&input.text, MAX_TEXT);
    let selector = clean_text(&input.selector, MAX_SELECTOR);
    let selector = if selector.is_empty() { tag.clone() } else { selector };

    // The opening tag: redact, then drop any value attribute (form values never leave), then cut.
    let html_redacted = redact::text(&input.html).into_inner();
    let classes = class_names(&html_redacted);
    let html = cut(&collapse(&drop_value_attrs(&html_redacted)), MAX_HTML);

    let attrs = clean_attrs(&input.attrs);
    let styles = clean_styles(&input.styles);
    let label = label_of(&tag, &attrs, &classes);
    let component = input.component.as_deref().map(|c| clean_text(c, MAX_TEXT)).filter(|c| !c.is_empty());
    let source_hint = input.source.and_then(clean_source);

    Ok(PickFacts {
        page,
        url,
        title,
        tag,
        label,
        selector,
        text,
        html,
        rect,
        viewport,
        attrs,
        styles,
        component,
        source_hint,
    })
}

/// Redact, collapse spaces, then cut. Redaction always comes before the cut.
fn clean_text(raw: &str, max: usize) -> String {
    cut(&collapse(&redact::text(raw).into_inner()), max)
}

/// The first `max` characters (not bytes), so the cut never splits a character.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lower-case tag name made of letters, digits and dashes. None when nothing is left.
fn clean_tag(raw: &str) -> Option<String> {
    let tag: String = redact::text(raw)
        .into_inner()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(MAX_TAG)
        .collect();
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

/// A usable viewport: finite and positive. The device pixel ratio is clamped to 0.5..=4 (1 when missing).
fn clean_viewport(v: &Viewport) -> Option<Viewport> {
    let usable = v.w.is_finite() && v.h.is_finite() && v.w > 0.0 && v.h > 0.0;
    if !usable {
        return None;
    }
    let dpr = if v.dpr.is_finite() && v.dpr > 0.0 { v.dpr.clamp(0.5, 4.0) } else { 1.0 };
    Some(Viewport { w: v.w.min(MAX_VIEWPORT_PX), h: v.h.min(MAX_VIEWPORT_PX), dpr })
}

fn finite_or_zero(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// Keep the rectangle inside the viewport: x and y within it, width and height never past its edge.
fn clamp_rect(r: &Rect, vp: &Viewport) -> Rect {
    let x = finite_or_zero(r.x).clamp(0.0, vp.w);
    let y = finite_or_zero(r.y).clamp(0.0, vp.h);
    let w = finite_or_zero(r.w).clamp(0.0, vp.w - x);
    let h = finite_or_zero(r.h).clamp(0.0, vp.h - y);
    Rect { x, y, w, h }
}

/// Only the allow-listed attributes, values redacted and capped, at most `MAX_ATTRS`.
fn clean_attrs(attrs: &[(String, String)]) -> Vec<(String, String)> {
    attrs
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim().to_ascii_lowercase();
            if !ATTR_KEYS.contains(&key.as_str()) {
                return None;
            }
            let value = if key == "href" { href_path(value) } else { value.clone() };
            let value = clean_text(&value, MAX_TEXT);
            if value.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .take(MAX_ATTRS)
        .collect()
}

/// An href as a path only: no scheme, host, query or fragment (a query can hold a token).
fn href_path(raw: &str) -> String {
    match tauri::Url::parse(raw) {
        Ok(url) => url.path().to_string(),
        Err(_) => raw.split(['#', '?']).next().unwrap_or("").to_string(),
    }
}

/// Only the allow-listed computed styles, values redacted and capped.
fn clean_styles(styles: &[(String, String)]) -> Vec<(String, String)> {
    styles
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim().to_ascii_lowercase();
            if !STYLE_KEYS.contains(&key.as_str()) {
                return None;
            }
            let value = clean_text(value, MAX_STYLE);
            if value.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .take(STYLE_KEYS.len())
        .collect()
}

/// A source hint with a file name. A `Stack` hint never keeps a line (the page gave the file only).
fn clean_source(hint: SourceHint) -> Option<SourceHint> {
    let file = clean_text(&hint.file, MAX_FILE);
    if file.is_empty() {
        return None;
    }
    let (line, column) = if hint.how == SourceHow::Stack { (None, None) } else { (hint.line, hint.column) };
    Some(SourceHint { file, line, column, how: hint.how })
}

/// `tag#id` when the element has an id, else `tag.class1.class2` (at most two classes).
fn label_of(tag: &str, attrs: &[(String, String)], classes: &[String]) -> String {
    if let Some((_, id)) = attrs.iter().find(|(key, _)| key == "id") {
        return format!("{tag}#{}", cut(id, MAX_NAME));
    }
    let mut label = tag.to_string();
    for class in classes {
        label.push('.');
        label.push_str(class);
    }
    label
}

static CLASS_ATTR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\sclass\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>"']+))"#).expect("class attribute pattern"));

static VALUE_ATTR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\svalue\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)"#).expect("value attribute pattern"));

/// The first two plain class names of an opening tag (letters, digits, dash, underscore).
fn class_names(html: &str) -> Vec<String> {
    let Some(caps) = CLASS_ATTR.captures(html) else {
        return Vec::new();
    };
    let value = caps.get(1).or(caps.get(2)).or(caps.get(3)).map(|m| m.as_str()).unwrap_or("");
    value
        .split_whitespace()
        .filter(|c| c.chars().count() <= MAX_NAME && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
        .take(2)
        .map(str::to_string)
        .collect()
}

/// Remove every `value="..."` attribute from an opening tag. `data-value` and `aria-valuetext` stay.
fn drop_value_attrs(html: &str) -> String {
    VALUE_ATTR.replace_all(html, "").into_owned()
}
