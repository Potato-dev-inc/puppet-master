//! The sealed screen links and the hub: tamper, expiry, generation, mode, lost lease, who it is for, the Labs
//! switches, the buttons and the viewer helpers. No clock, no sandbox, no socket: the clock is a number the test
//! moves, the sandbox is a struct that answers `target()`.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use super::lease::{LEASE_TTL_MS, NEEDS_YOU_TTL_MS};
use super::screen::*;
use super::secrets::ScreenSecrets;
use super::{SandboxError, SbxResult, ScreenTarget};

const T0: i64 = 5_000_000;
const PORT: u16 = 6080;

struct TestSource {
    name: String,
    generation: AtomicU64,
    secrets: Mutex<Option<Arc<ScreenSecrets>>>,
    fail: Mutex<Option<SandboxError>>,
    name_asked: AtomicU64,
}

impl TestSource {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            name: "pm-sbx-main".to_string(),
            generation: AtomicU64::new(1),
            secrets: Mutex::new(Some(Arc::new(ScreenSecrets::generate()))),
            fail: Mutex::new(None),
            name_asked: AtomicU64::new(0),
        })
    }

    fn secrets(&self) -> Arc<ScreenSecrets> {
        self.secrets.lock().clone().expect("running")
    }
}

impl ScreenSource for TestSource {
    fn name(&self) -> String {
        self.name_asked.fetch_add(1, Ordering::SeqCst);
        self.name.clone()
    }

    fn target(&self) -> SbxResult<ScreenTarget> {
        if let Some(e) = self.fail.lock().take() {
            return Err(e);
        }
        Ok(ScreenTarget { view_port: PORT, generation: self.generation.load(Ordering::SeqCst), passwords: self.secrets.lock().clone() })
    }
}

struct Rig {
    hub: Arc<ScreenHub>,
    source: Arc<TestSource>,
    clock: Arc<AtomicI64>,
    events: Arc<Mutex<Vec<ScreenEvent>>>,
}

fn rig() -> Rig {
    let clock = Arc::new(AtomicI64::new(T0));
    let tick = clock.clone();
    let hub = Arc::new(ScreenHub::with_clock(Box::new(move || tick.load(Ordering::SeqCst))));
    let source = TestSource::new();
    hub.set_source(Some(source.clone()));
    hub.set_enabled(true);
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    hub.set_listener(Some(Arc::new(move |e: &ScreenEvent| sink.lock().push(e.clone()))));
    events.lock().clear();
    Rig { hub, source, clock, events }
}

impl Rig {
    fn advance(&self, ms: i64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
    }

    /// The sealed part of an iframe address.
    fn sealed(src: ScreenSrc) -> String {
        let text = src.into_inner();
        let marker = "/screen/";
        let start = text.find(marker).expect("a screen path") + marker.len();
        text[start..].split('/').next().unwrap().to_string()
    }

    fn view(&self, audience: &Audience) -> String {
        Self::sealed(self.hub.link("fish", Mode::View, audience, "").expect("view link"))
    }

    /// The person takes over and gets a control link.
    fn control(&self, audience: &Audience) -> String {
        self.hub.take_over().expect("take over");
        Self::sealed(self.hub.link("fish", Mode::Control, audience, "").expect("control link"))
    }
}

fn app() -> Audience {
    Audience::App
}

fn phone(id: &str) -> Audience {
    Audience::Device(id.to_string())
}

// ---- the link and its address --------------------------------------------------------------------

#[test]
fn a_view_link_opens_and_points_at_the_view_port() {
    let r = rig();
    let sealed = r.view(&app());
    let grant = r.hub.open(&sealed, &app()).expect("opens");
    assert_eq!(grant.port, PORT);
    assert_eq!(grant.claims.m, Mode::View);
    assert_eq!(grant.claims.b, "fish");
    assert_eq!(grant.claims.s, "pm-sbx-main");
    assert_eq!(grant.claims.x, T0 + LINK_TTL_MS);
}

#[test]
fn the_password_is_only_in_the_fragment_and_the_mode_picks_which() {
    let r = rig();
    let secrets = r.source.secrets();
    let view = r.hub.link("fish", Mode::View, &app(), "http://127.0.0.1:1").unwrap().into_inner();
    let (before, fragment) = view.split_once('#').expect("a fragment");
    assert_eq!(fragment, format!("vnc={}", secrets.view().expose()));
    assert!(!before.contains(secrets.view().expose()) && !before.contains(secrets.control().expose()));
    assert!(before.starts_with("http://127.0.0.1:1/bridge/potato/sandbox/screen/"));
    assert!(before.ends_with("/embed.html"), "a view link has no ?control: {before}");
    assert!(!before.contains('?'), "no query at all on a view link: {before}");

    r.hub.take_over().unwrap();
    let control = r.hub.link("fish", Mode::Control, &app(), "").unwrap().into_inner();
    let (before, fragment) = control.split_once('#').unwrap();
    assert_eq!(fragment, format!("vnc={}", secrets.control().expose()));
    assert!(before.ends_with("/embed.html?control=1"), "{before}");
    assert!(!before.contains(secrets.control().expose()));
    // a phone's address is relative: its own origin
    let for_phone = r.hub.link("fish", Mode::View, &phone("d1"), "").unwrap().into_inner();
    assert!(for_phone.starts_with("/bridge/potato/sandbox/screen/"));
}

#[test]
fn nothing_that_holds_a_secret_prints_it() {
    let r = rig();
    let secrets = r.source.secrets();
    let src = r.hub.link("fish", Mode::View, &app(), "").unwrap();
    let shown = format!("{src:?} {:?} {:?}", ScreenKey::generate(), r.hub.state());
    assert!(!shown.contains(secrets.view().expose()));
    assert_eq!(format!("{src:?}"), "ScreenSrc(..)");
    assert_eq!(format!("{:?}", ScreenKey::generate()), "ScreenKey(..)");
}

// ---- tamper --------------------------------------------------------------------------------------

#[test]
fn a_changed_link_does_not_open() {
    let r = rig();
    let sealed = r.view(&app());
    assert!(r.hub.open(&sealed, &app()).is_ok());
    // every single character changed, one at a time
    for i in 0..sealed.len() {
        let mut bytes = sealed.clone().into_bytes();
        bytes[i] = if bytes[i] == b'A' { b'B' } else { b'A' };
        let changed = String::from_utf8(bytes).unwrap();
        if changed == sealed {
            continue;
        }
        assert_eq!(r.hub.open(&changed, &app()).err(), Some(ScreenError::LinkBad), "char {i}");
    }
    let dotless = sealed.replace('.', "");
    let shorter = &sealed[..sealed.len() - 1];
    let longer = format!("{sealed}.x");
    for bad in ["", ".", "abc", "abc.", ".abc", longer.as_str(), shorter, dotless.as_str()] {
        assert_eq!(r.hub.open(bad, &app()).err(), Some(ScreenError::LinkBad), "{bad}");
    }
    assert_eq!(r.hub.open(&"a".repeat(2000), &app()).err(), Some(ScreenError::LinkBad));
}

#[test]
fn a_link_from_another_run_of_the_app_does_not_open() {
    let a = rig();
    let b = rig();
    let sealed = a.view(&app());
    assert_eq!(b.hub.open(&sealed, &app()).err(), Some(ScreenError::LinkBad), "the key is new at every start");
}

#[test]
fn the_mode_cannot_be_changed_in_the_link() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let r = rig();
    let sealed = r.view(&app());
    let (payload, tag) = sealed.split_once('.').unwrap();
    let mut claims: Claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
    claims.m = Mode::Control;
    let forged = format!("{}.{tag}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()));
    assert_eq!(r.hub.open(&forged, &app()).err(), Some(ScreenError::LinkBad));
    // and a link sealed by someone else's key with the same claims is no better
    let other = ScreenKey::generate().seal(&claims);
    assert_eq!(r.hub.open(&other, &app()).err(), Some(ScreenError::LinkBad));
}

// ---- time, generation, sandbox -------------------------------------------------------------------

#[test]
fn a_link_runs_out_after_one_hour() {
    let r = rig();
    let sealed = r.view(&app());
    r.advance(LINK_TTL_MS - 1);
    assert!(r.hub.open(&sealed, &app()).is_ok());
    assert!(r.hub.peek_audience(&sealed).is_some());
    r.advance(1);
    assert_eq!(r.hub.open(&sealed, &app()).err(), Some(ScreenError::Expired));
    assert!(r.hub.peek_audience(&sealed).is_none());
}

#[test]
fn bumping_the_generation_kills_every_link() {
    let r = rig();
    let view = r.view(&app());
    let control = r.control(&app());
    let grant = r.hub.open(&view, &app()).unwrap();
    r.source.generation.fetch_add(1, Ordering::SeqCst);
    assert_eq!(r.hub.open(&view, &app()).err(), Some(ScreenError::Stale));
    assert_eq!(r.hub.open(&control, &app()).err(), Some(ScreenError::Stale));
    assert_eq!(r.hub.revalidate(&grant, &app()).err(), Some(ScreenError::Stale), "an open connection is closed too");
    // a new link for the new generation works
    let again = r.view(&app());
    assert!(r.hub.open(&again, &app()).is_ok());
}

#[test]
fn the_sandbox_name_is_asked_once() {
    let r = rig();
    let sealed = r.view(&app());
    for _ in 0..5 {
        r.hub.open(&sealed, &app()).unwrap();
    }
    assert_eq!(r.source.name_asked.load(Ordering::SeqCst), 1, "a real sandbox answers by asking the system");
}

#[test]
fn a_stopped_or_missing_sandbox_makes_no_link() {
    let r = rig();
    *r.source.secrets.lock() = None;
    assert_eq!(r.hub.link("fish", Mode::View, &app(), "").err(), Some(ScreenError::NotRunning));
    *r.source.fail.lock() = Some(SandboxError::new("SANDBOX_NOT_INSTALLED", "The sandbox is not set up yet."));
    let e = r.hub.link("fish", Mode::View, &app(), "").err().unwrap();
    assert_eq!(e.code(), "SANDBOX_NOT_INSTALLED");
    r.hub.set_source(None);
    assert_eq!(r.hub.link("fish", Mode::View, &app(), "").err().unwrap().code(), "SANDBOX_NOT_INSTALLED");
}

#[test]
fn a_bad_bot_name_is_refused() {
    let r = rig();
    let long = "x".repeat(65);
    for bad in ["", "a b", "a/b", "../x", "a\"b", long.as_str()] {
        assert_eq!(r.hub.link(bad, Mode::View, &app(), "").err(), Some(ScreenError::BadBot), "{bad:?}");
    }
    assert!(r.hub.link("bot-1_a", Mode::View, &app(), "").is_ok());
}

// ---- who it is for -------------------------------------------------------------------------------

#[test]
fn a_link_is_for_the_app_or_for_one_phone() {
    let r = rig();
    let for_app = r.view(&app());
    let for_phone = r.view(&phone("d1"));
    assert_eq!(r.hub.peek_audience(&for_app), Some(app()));
    assert_eq!(r.hub.peek_audience(&for_phone), Some(phone("d1")));
    assert_eq!(r.hub.open(&for_app, &phone("d1")).err(), Some(ScreenError::WrongAudience));
    assert_eq!(r.hub.open(&for_phone, &app()).err(), Some(ScreenError::WrongAudience));
    assert_eq!(r.hub.open(&for_phone, &phone("d2")).err(), Some(ScreenError::WrongAudience));
    assert!(r.hub.open(&for_phone, &phone("d1")).is_ok());
}

// ---- control and the lease -----------------------------------------------------------------------

#[test]
fn a_control_link_needs_the_person_to_have_taken_over() {
    let r = rig();
    assert_eq!(r.hub.link("fish", Mode::Control, &app(), "").err(), Some(ScreenError::NeedTakeOver));
    // a bot holding the screen is not the person
    r.hub.lease().lock().take_for_bot("fish", "run-1", T0).unwrap();
    assert_eq!(r.hub.link("fish", Mode::Control, &app(), "").err(), Some(ScreenError::NeedTakeOver));
    r.hub.take_over().unwrap();
    assert!(r.hub.link("fish", Mode::Control, &app(), "").is_ok());
}

#[test]
fn a_control_link_closes_when_the_lease_is_handed_back() {
    let r = rig();
    let control = r.control(&app());
    let grant = r.hub.open(&control, &app()).unwrap();
    assert_eq!(grant.claims.m, Mode::Control);
    assert!(r.hub.revalidate(&grant, &app()).is_ok());
    r.hub.hand_back().unwrap();
    assert_eq!(r.hub.revalidate(&grant, &app()).err(), Some(ScreenError::LeaseLost));
    assert_eq!(r.hub.open(&control, &app()).err(), Some(ScreenError::LeaseLost));
}

#[test]
fn a_new_take_over_is_a_new_fence_and_the_old_control_link_is_dead() {
    let r = rig();
    let first = r.control(&app());
    let second = r.control(&app());
    assert_eq!(r.hub.open(&first, &app()).err(), Some(ScreenError::LeaseLost));
    assert!(r.hub.open(&second, &app()).is_ok());
}

#[test]
fn a_control_link_closes_when_the_lease_runs_out_and_keep_extends_it() {
    let r = rig();
    let control = r.control(&app());
    let grant = r.hub.open(&control, &app()).unwrap();
    r.advance(LEASE_TTL_MS - 1000);
    r.hub.keep().unwrap();
    r.advance(LEASE_TTL_MS - 1000);
    assert!(r.hub.revalidate(&grant, &app()).is_ok(), "kept alive");
    r.advance(1000);
    assert_eq!(r.hub.revalidate(&grant, &app()).err(), Some(ScreenError::LeaseLost));
    assert_eq!(r.hub.keep().err(), Some(ScreenError::LeaseLost), "nothing left to keep");
}

#[test]
fn keep_is_only_for_the_person() {
    let r = rig();
    r.hub.lease().lock().take_for_bot("fish", "run-1", T0).unwrap();
    assert_eq!(r.hub.keep().err(), Some(ScreenError::LeaseLost));
}

#[test]
fn a_view_link_does_not_depend_on_the_lease() {
    let r = rig();
    let view = r.view(&app());
    let grant = r.hub.open(&view, &app()).unwrap();
    r.hub.lease().lock().take_for_bot("fish", "run-1", T0).unwrap();
    assert!(r.hub.revalidate(&grant, &app()).is_ok());
    r.hub.take_over().unwrap();
    assert!(r.hub.revalidate(&grant, &app()).is_ok());
    r.hub.hand_back().unwrap();
    assert!(r.hub.revalidate(&grant, &app()).is_ok());
}

// ---- the phone's yes -----------------------------------------------------------------------------

#[test]
fn a_phone_gets_view_but_control_needs_the_desktop_yes() {
    let r = rig();
    assert!(r.hub.link("fish", Mode::View, &phone("d1"), "").is_ok());
    r.hub.take_over().unwrap();
    let e = r.hub.link("fish", Mode::Control, &phone("d1"), "").err().unwrap();
    assert_eq!(e, ScreenError::PhoneNeedsDesktopYes);
    assert_eq!(e.code(), "PHONE_NEEDS_DESKTOP_YES");
    assert_eq!(e.status(), 403);

    r.hub.allow_phone_control("d1");
    let sealed = Rig::sealed(r.hub.link("fish", Mode::Control, &phone("d1"), "").unwrap());
    let grant = r.hub.open(&sealed, &phone("d1")).unwrap();
    // the yes is per device
    assert_eq!(r.hub.link("fish", Mode::Control, &phone("d2"), "").err(), Some(ScreenError::PhoneNeedsDesktopYes));
    // and taking it back closes the open connection
    r.hub.revoke_phone_control("d1");
    assert_eq!(r.hub.revalidate(&grant, &phone("d1")).err(), Some(ScreenError::PhoneNeedsDesktopYes));
}

// ---- the Labs switches ---------------------------------------------------------------------------

#[test]
fn with_the_switches_off_nothing_works_and_open_links_close() {
    let r = rig();
    let sealed = r.view(&app());
    let grant = r.hub.open(&sealed, &app()).unwrap();
    r.hub.set_enabled(false);
    assert_eq!(r.hub.open(&sealed, &app()).err(), Some(ScreenError::Off));
    assert_eq!(r.hub.revalidate(&grant, &app()).err(), Some(ScreenError::Off));
    assert_eq!(r.hub.link("fish", Mode::View, &app(), "").err(), Some(ScreenError::Off));
    assert_eq!(r.hub.take_over().err(), Some(ScreenError::Off));
    assert_eq!(r.hub.hand_back().err(), Some(ScreenError::Off));
    assert_eq!(r.hub.skip("needs-1").err(), Some(ScreenError::Off));
    assert!(!r.hub.state().enabled);
    r.hub.set_enabled(true);
    assert!(r.hub.open(&sealed, &app()).is_ok());
}

#[test]
fn a_hub_starts_off_and_needs_both_switches() {
    use serde_json::json;
    assert!(!ScreenHub::new().enabled());
    assert!(!super::screen_live::labs_on(&json!({})));
    assert!(!super::screen_live::labs_on(&json!({ "potato_bot_enabled": true })));
    assert!(!super::screen_live::labs_on(&json!({ "potato_sandbox_enabled": true })));
    assert!(!super::screen_live::labs_on(&json!({ "potato_bot_enabled": true, "potato_sandbox_enabled": "true" })));
    assert!(super::screen_live::labs_on(&json!({ "potato_bot_enabled": true, "potato_sandbox_enabled": true })));
}

// ---- the panel's state and buttons ---------------------------------------------------------------

#[test]
fn the_state_names_who_has_the_screen() {
    let r = rig();
    let s = r.hub.state();
    assert_eq!((s.phase, s.bot.clone(), s.card_id.clone()), ("free", None, None));
    let id = r.hub.lease().lock().take_for_bot("fish", "run-1", T0).unwrap();
    let s = r.hub.state();
    assert_eq!((s.phase, s.bot.as_deref()), ("bot", Some("fish")));
    r.hub.lease().lock().ask_for_help(&id, "A login code is needed", T0).unwrap();
    let s = r.hub.state();
    assert_eq!((s.phase, s.bot.as_deref(), s.card_id.as_deref()), ("needs_you", Some("fish"), Some("needs-1")));
    assert_eq!(s.reason.as_deref(), Some("A login code is needed"));
    assert_eq!(s.card_expires_ms, Some(T0 + NEEDS_YOU_TTL_MS));
    let s = r.hub.take_over().unwrap();
    assert_eq!((s.phase, s.bot, s.card_id), ("you", None, None));
    let s = r.hub.hand_back().unwrap();
    assert_eq!(s.phase, "free");
}

#[test]
fn the_state_serializes_in_camel_case() {
    let r = rig();
    let value = serde_json::to_value(r.hub.state()).unwrap();
    assert_eq!(value["phase"], "free");
    assert_eq!(value["enabled"], true);
    assert!(value.get("cardId").is_some() && value.get("cardExpiresMs").is_some());
}

#[test]
fn take_over_hand_back_and_skip_tell_the_listener_who_was_displaced() {
    let r = rig();
    let id = r.hub.lease().lock().take_for_bot("fish", "run-1", T0).unwrap();
    r.hub.lease().lock().ask_for_help(&id, "Solve the check", T0).unwrap();
    let displaced = super::lease::Displaced { bot: "fish".into(), run_id: "run-1".into() };
    r.hub.take_over().unwrap();
    r.hub.hand_back().unwrap();
    assert_eq!(
        *r.events.lock(),
        vec![ScreenEvent::TakenOver { displaced: Some(displaced.clone()) }, ScreenEvent::HandedBack { displaced: Some(displaced) }]
    );

    // skip: the card goes, the bot keeps the screen
    let r = rig();
    let id = r.hub.lease().lock().take_for_bot("fish", "run-2", T0).unwrap();
    let card = r.hub.lease().lock().ask_for_help(&id, "x", T0).unwrap();
    assert_eq!(r.hub.skip("needs-99").err(), Some(ScreenError::LeaseLost), "no such card");
    let s = r.hub.skip(&card.card_id).unwrap();
    assert_eq!(s.phase, "bot");
    assert_eq!(*r.events.lock(), vec![ScreenEvent::Skipped { bot: "fish".into(), run_id: "run-2".into() }]);
    assert_eq!(r.hub.skip(&card.card_id).err(), Some(ScreenError::LeaseLost), "a card is answered once");
}

#[test]
fn the_keeper_tells_the_listener_only_when_something_changed() {
    let r = rig();
    r.hub.tick(); // the first look is not news
    r.hub.tick();
    assert!(r.events.lock().is_empty());
    r.hub.lease().lock().take_for_bot("fish", "run-1", T0).unwrap();
    r.hub.tick();
    assert_eq!(*r.events.lock(), vec![ScreenEvent::Changed]);
    r.hub.tick();
    assert_eq!(r.events.lock().len(), 1, "nothing new");
    r.advance(LEASE_TTL_MS);
    r.hub.tick();
    assert_eq!(r.events.lock().len(), 2, "the lease ran out");
    assert_eq!(r.hub.state().phase, "free");
}

#[test]
fn turning_the_switch_tells_the_listener() {
    let r = rig();
    r.hub.set_enabled(true); // no change
    assert!(r.events.lock().is_empty());
    r.hub.set_enabled(false);
    assert_eq!(*r.events.lock(), vec![ScreenEvent::Changed]);
}

// ---- the viewer helpers --------------------------------------------------------------------------

#[test]
fn only_the_viewers_own_files_are_named() {
    assert_eq!(viewer_file(&[]).as_deref(), Some("embed.html"));
    assert_eq!(viewer_file(&["embed.html"]).as_deref(), Some("embed.html"));
    assert_eq!(viewer_file(&["clipboard-bridge.js"]).as_deref(), Some("clipboard-bridge.js"));
    assert_eq!(viewer_file(&["mobile-keyboard.js"]).as_deref(), Some("mobile-keyboard.js"));
    assert_eq!(viewer_file(&["novnc", "core", "rfb.js"]).as_deref(), Some("novnc/core/rfb.js"));
    assert_eq!(
        viewer_file(&["novnc", "vendor", "pako", "lib", "zlib", "inflate.js"]).as_deref(),
        Some("novnc/vendor/pako/lib/zlib/inflate.js")
    );
    let long = "a".repeat(61);
    for bad in [
        vec!["README.md"],
        vec!["PORTED.md"],
        vec!["novnc"],
        vec!["novnc", "..", "embed.html"],
        vec!["novnc", ".", "x.js"],
        vec!["novnc", ".hidden"],
        vec!["novnc", "core", "a b.js"],
        vec!["novnc", "core\\rfb.js"],
        vec!["novnc", "C:", "x.js"],
        vec!["novnc", "%2e%2e", "x.js"],
        vec!["", "novnc"],
        vec!["websockify"],
        vec!["embed.html", "x"],
        vec!["novnc", long.as_str()],
    ] {
        assert_eq!(viewer_file(&bad), None, "{bad:?}");
    }
}

#[test]
fn content_types_are_set_so_modules_load() {
    assert_eq!(content_type("novnc/core/rfb.js"), "text/javascript; charset=utf-8");
    assert_eq!(content_type("embed.html"), "text/html; charset=utf-8");
    assert_eq!(content_type("x.bin"), "application/octet-stream");
}

#[test]
fn the_app_origins_are_always_allowed_and_a_phone_adds_its_own() {
    let local = "GET /x HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nReferer: http://localhost:1420/\r\n";
    assert_eq!(parent_origins(local, false), vec!["http://localhost:1420", "http://tauri.localhost", "https://tauri.localhost"]);
    // a referer that is not this computer is not trusted
    let evil = "GET /x HTTP/1.1\r\nHost: 127.0.0.1:1\r\nReferer: https://evil.example/page\r\n";
    assert_eq!(parent_origins(evil, false), vec!["http://tauri.localhost", "https://tauri.localhost"]);
    // a phone: the tunnel rewrites Host, so the page that framed it (Referer) and the forwarded host say who it is
    let remote = "GET /x HTTP/1.1\r\nHost: 127.0.0.1:9\r\nX-Forwarded-Host: pm.example.test\r\nX-Forwarded-Proto: https\r\nReferer: https://pm.example.test/phone\r\n";
    assert_eq!(parent_origins(remote, true), vec!["http://tauri.localhost", "https://pm.example.test", "https://tauri.localhost"]);
    let upgrade = "GET /x HTTP/1.1\r\nHost: pm.example.test:8443\r\nX-Forwarded-Proto: https\r\n";
    assert!(parent_origins(upgrade, true).contains(&"https://pm.example.test:8443".to_string()));
    // the same Referer on a request that is not from a phone adds nothing
    assert!(!parent_origins(remote, false).iter().any(|o| o.contains("pm.example.test")));
    // a weird host is dropped, never trusted
    assert!(!parent_origins("GET / HTTP/1.1\r\nHost: a b\r\n", true).iter().any(|o| o.contains(' ')));
    assert!(!parent_origins("GET / HTTP/1.1\r\nHost: x.test/<>\r\n", true).iter().any(|o| o.contains('<')));
}

#[test]
fn the_page_gets_its_allowlist_and_nothing_else_changes() {
    let html = r#"<head><meta name="pm-sbx-parent-origins" content="" /><title>x</title></head>"#;
    let out = fill_viewer_page(html, &["http://tauri.localhost".to_string(), "https://a.test".to_string()]);
    assert!(out.contains(r#"content="http://tauri.localhost,https://a.test""#));
    assert!(out.contains("<title>x</title>"));
    // no tag: unchanged (the page then posts nothing)
    assert_eq!(fill_viewer_page("<p>none</p>", &["http://a.test".to_string()]), "<p>none</p>");
    // the real page still has the empty tag this fills
    let viewer = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox").join("viewer");
    let real = std::fs::read_to_string(viewer.join("embed.html")).unwrap();
    assert!(fill_viewer_page(&real, &["http://tauri.localhost".to_string()]).contains(r#"content="http://tauri.localhost""#));
    assert_eq!(frame_policy(&["http://a.test".to_string()]), "frame-ancestors 'self' http://a.test");
}

#[test]
fn route_parts_cut_the_sealed_segment() {
    assert_eq!(route_parts(&["potato", "sandbox", "screen", "S", "embed.html"]), Some(("S", &["embed.html"][..])));
    assert_eq!(route_parts(&["bridge", "potato", "sandbox", "screen", "S", "novnc", "core", "rfb.js"]), Some(("S", &["novnc", "core", "rfb.js"][..])));
    assert_eq!(route_parts(&["potato", "sandbox", "screen", "S"]), Some(("S", &[][..])));
    assert_eq!(route_parts(&["potato", "sandbox", "screen"]), None);
    assert_eq!(route_parts(&["potato", "sandbox", "screen-link"]), None);
    assert_eq!(route_parts(&["potato", "brain", "inbox"]), None);
}
