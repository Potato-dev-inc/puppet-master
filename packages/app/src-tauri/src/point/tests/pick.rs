//! Point and Fix tests for pick (PF-2): caps, redaction, origin refusal, clamping, allow-lists.
use crate::point::origin::PreviewOrigin;
use crate::point::pick::{sanitize, PickInput, PickRefusal};
use crate::point::{Rect, SourceHint, SourceHow, Viewport, MAX_HTML, MAX_SELECTOR, MAX_TEXT, MAX_TITLE};

fn origin() -> PreviewOrigin {
    PreviewOrigin { scheme: "http".into(), host: "localhost".into(), port: 5173 }
}

/// A normal pick on the fixture page, with every field set to something plausible.
fn input() -> PickInput {
    PickInput {
        url: "http://localhost:5173/pricing".into(),
        title: "Pricing - Acme".into(),
        tag: "button".into(),
        selector: "main > div.card:nth-of-type(2) > button.card-cta".into(),
        text: "Get started".into(),
        html: r#"<button class="card-cta primary" type="button">"#.into(),
        rect: Rect { x: 612.0, y: 380.0, w: 180.0, h: 44.0 },
        viewport: Viewport { w: 1280.0, h: 800.0, dpr: 1.5 },
        attrs: vec![("data-testid".into(), "buy".into())],
        styles: vec![("font-size".into(), "14px".into()), ("color".into(), "rgb(255, 255, 255)".into())],
        component: Some("PricingCard".into()),
        source: None,
    }
}

fn clean(i: PickInput) -> crate::point::PickFacts {
    sanitize(i, &origin()).expect("pick should be accepted")
}

#[test]
fn normal_pick_keeps_its_facts() {
    let f = clean(input());
    assert_eq!(f.page, "localhost:5173/pricing");
    assert_eq!(f.url, "http://localhost:5173/pricing");
    assert_eq!(f.title, "Pricing - Acme");
    assert_eq!(f.tag, "button");
    assert_eq!(f.label, "button.card-cta.primary");
    assert_eq!(f.text, "Get started");
    assert_eq!(f.component.as_deref(), Some("PricingCard"));
    assert_eq!(f.viewport.dpr, 1.5);
    assert_eq!(f.attrs, vec![("data-testid".to_string(), "buy".to_string())]);
    assert_eq!(f.rect.x, 612.0);
    assert_eq!(f.rect.w, 180.0);
}

#[test]
fn text_title_html_and_selector_are_capped_by_characters() {
    let mut i = input();
    i.text = "é".repeat(500);
    i.title = "t".repeat(500);
    i.html = format!("<p class=\"x\">{}</p>", "é".repeat(900));
    i.selector = "s".repeat(900);
    let f = clean(i);
    assert_eq!(f.text.chars().count(), MAX_TEXT);
    assert_eq!(f.title.chars().count(), MAX_TITLE);
    assert!(f.html.chars().count() <= MAX_HTML);
    assert_eq!(f.selector.chars().count(), MAX_SELECTOR);
    assert!(f.text.chars().all(|c| c == 'é'), "cut must stay on a character boundary");
}

#[test]
fn text_whitespace_is_collapsed() {
    let mut i = input();
    i.text = "  Get \n\t started  ".into();
    assert_eq!(clean(i).text, "Get started");
}

#[test]
fn fragment_is_dropped_from_the_url() {
    let mut i = input();
    i.url = "http://localhost:5173/pricing?plan=pro#section-secret-anchor".into();
    let f = clean(i);
    assert!(!f.url.contains('#'), "fragment must be gone: {}", f.url);
    assert!(!f.url.contains("secret-anchor"));
    assert!(f.url.contains("plan=pro"));
    assert_eq!(f.page, "localhost:5173/pricing", "page address has no query");
}

#[test]
fn token_in_the_query_is_redacted() {
    let mut i = input();
    i.url = "http://localhost:5173/pricing?token=sk-ant-api03-abcdefghijklmnopqrstuv".into();
    let f = clean(i);
    assert!(!f.url.contains("abcdefghijklmnopqrstuv"), "token must not survive: {}", f.url);
    assert!(f.url.contains("pricing"));
}

#[test]
fn secret_looking_text_is_redacted() {
    let mut i = input();
    i.text = "Key sk-ant-api03-abcdefghijklmnopqrstuv".into();
    i.title = "Settings sk-ant-api03-abcdefghijklmnopqrstuv".into();
    let f = clean(i);
    assert!(!f.text.contains("abcdefghijklmnopqrstuv"));
    assert!(!f.title.contains("abcdefghijklmnopqrstuv"));
}

#[test]
fn foreign_origin_is_refused() {
    let mut i = input();
    i.url = "https://example.com/pricing".into();
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::OtherOrigin);

    let mut i = input();
    i.url = "http://localhost:5174/pricing".into();
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::OtherOrigin, "other port");

    let mut i = input();
    i.url = "http://evil@localhost:5173/pricing".into();
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::OtherOrigin, "user name in the URL");

    let mut i = input();
    i.url = "https://localhost:5173/pricing".into();
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::OtherOrigin, "other scheme");
}

#[test]
fn unreadable_url_is_bad_input() {
    let mut i = input();
    i.url = "not a url".into();
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::BadInput);
}

#[test]
fn refusal_messages_are_plain_sentences() {
    assert!(PickRefusal::OtherOrigin.message().ends_with('.'));
    assert!(PickRefusal::BadInput.message().ends_with('.'));
}

#[test]
fn rect_nan_negative_and_oversize_are_clamped() {
    let mut i = input();
    i.rect = Rect { x: f64::NAN, y: -5.0, w: 1e9, h: -3.0 };
    let f = clean(i);
    assert_eq!(f.rect.x, 0.0);
    assert_eq!(f.rect.y, 0.0);
    assert_eq!(f.rect.w, 1280.0, "width stops at the viewport edge");
    assert_eq!(f.rect.h, 0.0, "negative height becomes zero");
}

#[test]
fn rect_past_the_edge_is_pulled_back_inside() {
    let mut i = input();
    i.rect = Rect { x: 1000.0, y: 790.0, w: 1e9, h: 1e9 };
    let f = clean(i);
    assert_eq!(f.rect.x, 1000.0);
    assert_eq!(f.rect.w, 280.0);
    assert_eq!(f.rect.y, 790.0);
    assert_eq!(f.rect.h, 10.0);
}

#[test]
fn bad_viewport_is_refused_and_dpr_is_clamped() {
    let mut i = input();
    i.viewport = Viewport { w: f64::NAN, h: 800.0, dpr: 1.0 };
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::BadInput);

    let mut i = input();
    i.viewport = Viewport { w: 0.0, h: 800.0, dpr: 1.0 };
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::BadInput);

    let mut i = input();
    i.viewport = Viewport { w: 1280.0, h: 800.0, dpr: 0.0 };
    assert_eq!(clean(i).viewport.dpr, 1.0, "missing ratio is 1");

    let mut i = input();
    i.viewport = Viewport { w: 1e9, h: 800.0, dpr: 100.0 };
    let f = clean(i);
    assert_eq!(f.viewport.dpr, 4.0);
    assert_eq!(f.viewport.w, 16_384.0);
}

#[test]
fn attributes_are_capped_at_eight_and_allow_listed() {
    let mut i = input();
    i.attrs = (0..10).map(|n| ("data-testid".to_string(), format!("item-{n}"))).collect();
    i.attrs.push(("onclick".into(), "steal()".into()));
    i.attrs.push(("value".into(), "hunter2".into()));
    let f = clean(i);
    assert_eq!(f.attrs.len(), 8);
    assert!(f.attrs.iter().all(|(k, _)| k == "data-testid"));
}

#[test]
fn href_keeps_only_the_path() {
    let mut i = input();
    i.attrs = vec![
        ("href".into(), "https://app.example.test/reset?token=sk-ant-api03-abcdefghijklmnopqrstuv#x".into()),
        ("href".into(), "/docs/intro?code=123#top".into()),
    ];
    let f = clean(i);
    assert_eq!(f.attrs[0], ("href".to_string(), "/reset".to_string()));
    assert_eq!(f.attrs[1], ("href".to_string(), "/docs/intro".to_string()));
}

#[test]
fn styles_are_allow_listed() {
    let mut i = input();
    i.styles = vec![("font-size".into(), "14px".into()), ("position".into(), "fixed".into()), ("z-index".into(), "99".into())];
    let f = clean(i);
    assert_eq!(f.styles, vec![("font-size".to_string(), "14px".to_string())]);
}

#[test]
fn form_values_never_reach_the_html() {
    let mut i = input();
    i.tag = "input".into();
    i.text = String::new();
    i.html = r#"<input type="password" value="hunter2" class="pw" data-value="kept">"#.into();
    let f = clean(i);
    assert!(!f.html.contains("hunter2"), "value must be gone: {}", f.html);
    assert!(f.html.contains(r#"data-value="kept""#));
    assert!(f.html.contains("type=\"password\""));
}

#[test]
fn unquoted_form_value_is_dropped_too() {
    let mut i = input();
    i.html = "<input type=text value=secret-word class=field>".into();
    let f = clean(i);
    assert!(!f.html.contains("secret-word"), "{}", f.html);
    assert!(f.html.contains("class=field"));
}

#[test]
fn label_uses_the_id_then_two_classes() {
    let mut i = input();
    i.attrs = vec![("id".into(), "buy-now".into())];
    assert_eq!(clean(i).label, "button#buy-now");

    let mut i = input();
    i.html = r#"<a class="nav one two three" href="/">"#.into();
    i.tag = "a".into();
    assert_eq!(clean(i).label, "a.nav.one");
}

#[test]
fn tag_is_lower_cased_and_must_not_be_empty() {
    let mut i = input();
    i.tag = "BUTTON".into();
    assert_eq!(clean(i).tag, "button");

    let mut i = input();
    i.tag = "!!!".into();
    assert_eq!(sanitize(i, &origin()).unwrap_err(), PickRefusal::BadInput);
}

#[test]
fn empty_selector_falls_back_to_the_tag() {
    let mut i = input();
    i.selector = "   ".into();
    assert_eq!(clean(i).selector, "button");
}

#[test]
fn component_is_trimmed_and_empty_becomes_none() {
    let mut i = input();
    i.component = Some("  PricingCard  ".into());
    assert_eq!(clean(i).component.as_deref(), Some("PricingCard"));

    let mut i = input();
    i.component = Some("   ".into());
    assert_eq!(clean(i).component, None);
}

#[test]
fn stack_source_keeps_the_file_but_never_a_line() {
    let mut i = input();
    i.source = Some(SourceHint { file: "src/components/PricingCard.tsx".into(), line: Some(42), column: Some(7), how: SourceHow::Stack });
    let f = clean(i);
    let hint = f.source_hint.expect("file kept");
    assert_eq!(hint.file, "src/components/PricingCard.tsx");
    assert_eq!(hint.line, None);
    assert_eq!(hint.column, None);
}

#[test]
fn react_source_keeps_its_line() {
    let mut i = input();
    i.source = Some(SourceHint { file: "src/Card.jsx".into(), line: Some(12), column: Some(3), how: SourceHow::ReactDebugSource });
    let hint = clean(i).source_hint.expect("kept");
    assert_eq!(hint.line, Some(12));
    assert_eq!(hint.how, SourceHow::ReactDebugSource);
}

#[test]
fn empty_source_file_is_dropped() {
    let mut i = input();
    i.source = Some(SourceHint { file: "  ".into(), line: Some(1), column: None, how: SourceHow::DataAttr });
    assert!(clean(i).source_hint.is_none());
}

#[test]
fn unknown_source_how_is_rejected_by_serde() {
    let json = r#"{
        "url": "http://localhost:5173/", "title": "", "tag": "div", "selector": "div", "text": "",
        "html": "<div>", "rect": {"x": 0, "y": 0, "w": 1, "h": 1},
        "viewport": {"w": 100, "h": 100, "dpr": 1}, "attrs": [], "styles": [],
        "component": null,
        "source": {"file": "src/a.tsx", "line": 1, "column": null, "how": "bogus"}
    }"#;
    assert!(serde_json::from_str::<PickInput>(json).is_err());
}

#[test]
fn known_source_how_parses_from_json() {
    let json = r#"{
        "url": "http://localhost:5173/", "title": "", "tag": "div", "selector": "div", "text": "",
        "html": "<div>", "rect": {"x": 0, "y": 0, "w": 1, "h": 1},
        "viewport": {"w": 100, "h": 100, "dpr": 1}, "attrs": [], "styles": [],
        "component": null,
        "source": {"file": "src/a.tsx", "line": 1, "column": null, "how": "stack"}
    }"#;
    let parsed: PickInput = serde_json::from_str(json).expect("parses");
    let f = sanitize(parsed, &origin()).expect("accepted");
    assert_eq!(f.source_hint.map(|h| h.how), Some(SourceHow::Stack));
}
