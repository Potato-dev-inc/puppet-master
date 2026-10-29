//! `GET /focus`, `POST /focus/start`, `POST /focus/extend` and `POST /focus/end`: Focus on the phone (SPEC n33 3.5,
//! section 4 S1, S5, S6).
//!
//! The read is the phone view (`PhoneView`): counts, the end time and one counts-only line. It has no pane id, no
//! agent name and no held sentence. The writes go to the engine (`attention::focus::live::engine`), which keeps the
//! rules: the 5 to 240 minute check, the 240 minute cap and the person-only check. The engine also writes the audit
//! line (kind `focus`, counts only), so this file writes none of its own.
//!
//! Who may call:
//! - A paired phone, by its caps from `route_need`: `watch` to read (the default of a GET) and `control` to start,
//!   extend or end (the default of a POST). Without them the shared auth refuses before this file.
//! - A plain program on this computer with no forwarder header, no foreign `Host` or `Origin` (`Caller::Local`).
//!   It may read and END. Start and extend are refused with 403 `FOCUS_PERSON_ONLY`.
//! - Never an MCP session (an agent): refused with 403 `PERSON_ONLY`, whatever it sends.
//!
//! Every request first goes through `authorize_caller`, so the bridge-auth invariant holds: a request with a
//! forwarder header, a foreign `Host` or a foreign `Origin` needs a paired token.

use serde_json::{json, Value};

use super::{Ctx, RouteResult};
use crate::attention::focus::live::engine;
use crate::attention::focus::model::{
    validate_minutes, By, FocusError, CODE_BAD_MINUTES, CODE_NOT_ON, CODE_PERSON_ONLY,
};
use crate::attention::focus::view::PhoneView;
use crate::mobile_pairing::{authorize_caller, normalize_bridge_segments, Caller};

/// What the route touches. The live one is [`Live`]; tests use a fake that records the calls.
pub(crate) trait FocusPort {
    /// The phone view at this moment (counts only).
    fn view(&self) -> PhoneView;
    /// Start Focus for `minutes` (already checked by the route), or change a Focus that is on.
    fn start(&self, minutes: u32, by: By) -> Result<PhoneView, FocusError>;
    /// `+15 min` on a Focus that is on.
    fn extend(&self, by: By) -> Result<PhoneView, FocusError>;
    /// End Focus now. Idempotent: while off it changes nothing.
    fn end(&self, by: By) -> PhoneView;
}

/// Who is on the other end, as the route sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Actor {
    /// A program on this computer (no pairing, no forwarder).
    Local,
    /// A paired phone.
    Phone,
}

impl Actor {
    fn by(self) -> By {
        match self {
            Actor::Local => By::Local,
            Actor::Phone => By::Phone,
        }
    }
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    handle_request(cx.method, cx.segments, cx.headers, cx.peer_loopback, cx.session, &cx.request_body, &Live)
}

/// `None` when the request is not one of the four Focus routes.
pub(crate) fn handle_request(
    method: &str,
    raw_segments: &[&str],
    headers: &str,
    peer_loopback: bool,
    session: Option<&str>,
    body: &Value,
    port: &dyn FocusPort,
) -> Option<RouteResult> {
    let segments = normalize_bridge_segments(raw_segments);
    let route = match (method, segments) {
        ("GET", ["focus"]) => Route::Read,
        ("POST", ["focus", "start"]) => Route::Start,
        ("POST", ["focus", "extend"]) => Route::Extend,
        ("POST", ["focus", "end"]) => Route::End,
        _ => return None,
    };
    let actor = match authorize_caller(headers, peer_loopback, raw_segments, method) {
        Ok(Caller::Local) => Actor::Local,
        Ok(Caller::Device { .. }) => Actor::Phone,
        Ok(Caller::Public) => return Some(person_only()),
        Err(refusal) => return Some(Err(refusal)),
    };
    if session.is_some() {
        return Some(person_only());
    }
    Some(match route {
        Route::Read => Ok((200, json_of(&port.view()))),
        Route::Start => start(actor, body, port),
        Route::Extend => extend(actor, port),
        Route::End => Ok((200, json_of(&port.end(actor.by())))),
    })
}

enum Route {
    Read,
    Start,
    Extend,
    End,
}

fn start(actor: Actor, body: &Value, port: &dyn FocusPort) -> RouteResult {
    if actor == Actor::Local {
        return Err(refusal(FocusError::person_only()));
    }
    let minutes = body
        .get("minutes")
        .and_then(Value::as_u64)
        .and_then(|m| u32::try_from(m).ok())
        .ok_or_else(FocusError::bad_minutes)
        .and_then(validate_minutes)
        .map_err(refusal)?;
    port.start(minutes, actor.by()).map(|view| (200, json_of(&view))).map_err(refusal)
}

fn extend(actor: Actor, port: &dyn FocusPort) -> RouteResult {
    if actor == Actor::Local {
        return Err(refusal(FocusError::person_only()));
    }
    port.extend(actor.by()).map(|view| (200, json_of(&view))).map_err(refusal)
}

fn json_of(view: &PhoneView) -> Value {
    serde_json::to_value(view).unwrap_or(Value::Null)
}

/// The error shape of every Focus refusal: a slug in `error`, the stable `code` and the plain message.
fn refusal(err: FocusError) -> (u16, Value) {
    let (status, slug) = match err.code {
        CODE_BAD_MINUTES => (400, "focus_bad_minutes"),
        CODE_NOT_ON => (409, "focus_not_on"),
        CODE_PERSON_ONLY => (403, "focus_person_only"),
        _ => (400, "focus_refused"),
    };
    (status, json!({ "error": slug, "code": err.code, "message": err.message }))
}

fn person_only() -> RouteResult {
    Err((
        403,
        json!({
            "error": "focus_person_only",
            "code": "PERSON_ONLY",
            "hint": "Focus is for the person. An agent cannot read, start, extend or end it.",
        }),
    ))
}

/// The live port: the engine of the app.
struct Live;

impl FocusPort for Live {
    fn view(&self) -> PhoneView {
        engine().phone_view()
    }

    fn start(&self, minutes: u32, by: By) -> Result<PhoneView, FocusError> {
        engine().start(minutes, by).map(|view| view.phone)
    }

    fn extend(&self, by: By) -> Result<PhoneView, FocusError> {
        engine().extend(by).map(|view| view.phone)
    }

    fn end(&self, by: By) -> PhoneView {
        engine().end(by).phone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::focus::summary::EndedBy;
    use crate::attention::focus::view::PhoneSummary;
    use crate::mobile_pairing::{init_pairing_store, pairing_store, Cap, Caps, Need, PairRequestBody, route_need};
    use std::cell::RefCell;

    /// Records what the route asked for, and answers from a fixed phone view.
    struct Fake {
        on: bool,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(on: bool) -> Fake {
            Fake { on, calls: RefCell::new(Vec::new()) }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl FocusPort for Fake {
        fn view(&self) -> PhoneView {
            phone_view(self.on)
        }

        fn start(&self, minutes: u32, by: By) -> Result<PhoneView, FocusError> {
            self.calls.borrow_mut().push(format!("start {minutes} {}", by_name(by)));
            Ok(phone_view(true))
        }

        fn extend(&self, by: By) -> Result<PhoneView, FocusError> {
            self.calls.borrow_mut().push(format!("extend {}", by_name(by)));
            if self.on {
                Ok(phone_view(true))
            } else {
                Err(FocusError::not_on())
            }
        }

        fn end(&self, by: By) -> PhoneView {
            self.calls.borrow_mut().push(format!("end {}", by_name(by)));
            phone_view(false)
        }
    }

    fn by_name(by: By) -> &'static str {
        match by {
            By::Desktop => "desktop",
            By::Tray => "tray",
            By::Phone => "phone",
            By::Local => "local",
        }
    }

    fn phone_view(on: bool) -> PhoneView {
        PhoneView {
            on,
            ends_at_ms: on.then_some(1_900_000_000_000),
            minutes: 45,
            held_total: 3,
            waiting: 1,
            last: Some(PhoneSummary {
                ended_at_ms: 1_800_000_000_000,
                line: "Focus ended: 4 turns done, 1 failed".into(),
                ended_by: EndedBy::Timer,
            }),
        }
    }

    const LOCAL: &str = "Host: 127.0.0.1:17321\r\n";
    const FORWARDED: &str = "Host: 127.0.0.1:17321\r\nX-Forwarded-For: 203.0.113.9\r\n";
    const FOREIGN_HOST: &str = "Host: evil.example.com\r\n";
    const FOREIGN_ORIGIN: &str = "Host: 127.0.0.1:17321\r\nOrigin: https://evil.example.com\r\n";

    fn call(
        method: &str,
        segments: &[&str],
        headers: &str,
        loopback: bool,
        session: Option<&str>,
        body: Value,
        port: &dyn FocusPort,
    ) -> (u16, Value) {
        match handle_request(method, segments, headers, loopback, session, &body, port).expect("a focus route") {
            Ok(ok) => ok,
            Err(err) => err,
        }
    }

    fn refused(result: (u16, Value), status: u16, code: &str) {
        assert_eq!(result.0, status, "{}", result.1);
        assert_eq!(result.1["code"], code, "{}", result.1);
    }

    /// A paired phone with `caps`, paired the same way the night route tests pair one.
    fn pair(caps: Caps) -> String {
        // The tests run in parallel and the store is one global. A racing check-then-init lets
        // two tests write the same file at once, so the loser's load fails and nothing is set.
        // `Once` makes the first caller build it and the rest wait for it.
        static STORE: std::sync::Once = std::sync::Once::new();
        STORE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("pm-focus-bridge-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            init_pairing_store(dir.join("pairing.json")).expect("a pairing store for the focus tests");
        });
        let store = pairing_store().unwrap();
        let mut guard = store.lock();
        let s = guard.create_pairing_session_with_caps("http://127.0.0.1:1".into(), caps);
        let paired = guard
            .pair_device(PairRequestBody { pairing_code: s.pairing_code, device_name: "Test phone".into(), device_public_key: s.server_public_key })
            .unwrap();
        paired.device_token
    }

    /// Headers of a request from a phone over the tunnel: a remote peer with its bearer token.
    fn phone_headers(token: &str) -> String {
        format!("Host: pm.example.com\r\nAuthorization: Bearer {token}\r\n")
    }

    #[test]
    fn the_read_is_the_phone_view_with_no_pane_id_or_sentence() {
        let fake = Fake::new(true);
        let (status, body) = call("GET", &["focus"], LOCAL, true, None, json!({}), &fake);
        assert_eq!(status, 200, "{body}");
        let text = body.to_string();
        // A JSON string escapes every backslash, so a path separator would show up here.
        assert!(!text.contains('\\'), "a backslash leaked: {text}");
        for leak in ["pane", "agent", "C:/", "/home/"] {
            assert!(!text.contains(leak), "{leak} leaked: {text}");
        }
        let mut keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["endsAtMs", "heldTotal", "last", "minutes", "on", "waiting"]);
        assert_eq!(body["last"]["line"], "Focus ended: 4 turns done, 1 failed");
        // The bridge prefix is the same route.
        assert_eq!(call("GET", &["bridge", "focus"], LOCAL, true, None, json!({}), &fake).0, 200);
    }

    #[test]
    fn a_paired_phone_starts_extends_and_ends_focus_and_the_port_is_told_phone() {
        let fake = Fake::new(true);
        let headers = phone_headers(&pair(Caps::all()));
        let (status, body) = call("POST", &["focus", "start"], &headers, false, None, json!({ "minutes": 25 }), &fake);
        assert_eq!(status, 200, "{body}");
        assert_eq!(call("POST", &["focus", "extend"], &headers, false, None, json!({}), &fake).0, 200);
        let (status, body) = call("POST", &["focus", "end"], &headers, false, None, json!({}), &fake);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["on"], false);
        assert_eq!(fake.calls(), ["start 25 phone", "extend phone", "end phone"]);
    }

    #[test]
    fn bad_minutes_is_400_and_the_port_is_never_asked() {
        let fake = Fake::new(false);
        let headers = phone_headers(&pair(Caps::all()));
        for bad in [json!({ "minutes": 0 }), json!({ "minutes": 4 }), json!({ "minutes": 241 }), json!({ "minutes": 300 }), json!({ "minutes": "25" }), json!({})] {
            let (status, body) = call("POST", &["focus", "start"], &headers, false, None, bad.clone(), &fake);
            assert_eq!((status, body["code"].as_str()), (400, Some("FOCUS_BAD_MINUTES")), "{bad}");
        }
        assert!(fake.calls().is_empty());
        // The edges of the range are fine.
        for good in [5, 240] {
            assert_eq!(call("POST", &["focus", "start"], &headers, false, None, json!({ "minutes": good }), &fake).0, 200);
        }
        assert_eq!(fake.calls(), ["start 5 phone", "start 240 phone"]);
    }

    #[test]
    fn a_plain_local_program_may_end_but_never_start_or_extend() {
        let fake = Fake::new(true);
        refused(call("POST", &["focus", "start"], LOCAL, true, None, json!({ "minutes": 25 }), &fake), 403, "FOCUS_PERSON_ONLY");
        refused(call("POST", &["focus", "extend"], LOCAL, true, None, json!({}), &fake), 403, "FOCUS_PERSON_ONLY");
        assert_eq!(call("POST", &["focus", "end"], LOCAL, true, None, json!({}), &fake).0, 200);
        assert_eq!(call("GET", &["focus"], LOCAL, true, None, json!({}), &fake).0, 200);
        assert_eq!(fake.calls(), ["end local"]);
    }

    #[test]
    fn an_mcp_session_is_refused_on_every_focus_route() {
        let fake = Fake::new(true);
        for (method, segments) in [("GET", &["focus"][..]), ("POST", &["focus", "start"][..]), ("POST", &["focus", "extend"][..]), ("POST", &["focus", "end"][..])] {
            refused(call(method, segments, LOCAL, true, Some("mcp-session-1"), json!({ "minutes": 25 }), &fake), 403, "PERSON_ONLY");
        }
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_forwarder_or_a_foreign_host_or_origin_without_a_token_is_refused() {
        let fake = Fake::new(true);
        for headers in [FORWARDED, FOREIGN_HOST, FOREIGN_ORIGIN] {
            assert_eq!(call("GET", &["focus"], headers, true, None, json!({}), &fake).0, 401, "{headers}");
            assert_eq!(call("POST", &["focus", "start"], headers, true, None, json!({ "minutes": 25 }), &fake).0, 401, "{headers}");
            assert_eq!(call("POST", &["focus", "extend"], headers, true, None, json!({}), &fake).0, 401, "{headers}");
            assert_eq!(call("POST", &["focus", "end"], headers, true, None, json!({}), &fake).0, 401, "{headers}");
        }
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_paired_phone_needs_watch_to_read_and_control_to_change() {
        assert_eq!(route_need("GET", &["focus"]), Need::Cap(Cap::Watch));
        for segments in [&["focus", "start"][..], &["focus", "extend"][..], &["focus", "end"][..]] {
            assert_eq!(route_need("POST", segments), Need::Cap(Cap::Control));
        }
        let fake = Fake::new(true);
        let watch_headers = phone_headers(&pair(Caps::watch_and_answer()));
        assert_eq!(call("GET", &["focus"], &watch_headers, false, None, json!({}), &fake).0, 200);
        refused(call("POST", &["focus", "end"], &watch_headers, false, None, json!({}), &fake), 403, "DEVICE_NOT_ALLOWED");
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn extend_when_focus_is_off_is_409_and_changes_nothing() {
        let fake = Fake::new(false);
        let headers = phone_headers(&pair(Caps::all()));
        refused(call("POST", &["focus", "extend"], &headers, false, None, json!({}), &fake), 409, "FOCUS_NOT_ON");
        assert_eq!(fake.calls(), ["extend phone"]);
    }

    #[test]
    fn other_routes_are_left_alone() {
        let fake = Fake::new(true);
        for (method, segments) in [
            ("POST", &["focus"][..]),
            ("GET", &["focus", "start"][..]),
            ("GET", &["focus", "end"][..]),
            ("POST", &["focus", "stop"][..]),
            ("GET", &["focuses"][..]),
            ("DELETE", &["focus"][..]),
        ] {
            assert!(handle_request(method, segments, LOCAL, true, None, &json!({}), &fake).is_none(), "{method} {segments:?}");
        }
        assert!(fake.calls().is_empty());
    }
}
