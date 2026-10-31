//! `GET /night` and `POST /night/stop`: Night Shift on the phone (SPEC n32 3.8, section 4 S2, S6, S7).
//!
//! Read and stop only. The read is the redacted `NightView` (`redacted_for_bridge`: no project
//! names, no pane ids, no turn sentences, no agent questions, titles cut). The stop asks the
//! supervisor to stop the run at its next tick; it never types into a pane and never merges.
//!
//! Who may call:
//! - A paired phone, by its caps from `route_need`: `watch` to read (the default of a GET) and
//!   `control` to stop (the default of a POST). Without them the router answers 403 before this file.
//! - A plain program on this computer with no forwarder header, no foreign `Host` or `Origin`
//!   (`Caller::Local`). It may read and stop, and the stop is audited as `local`.
//! - Never an MCP session (an agent): refused with 403, whatever it sends.
//!
//! Every request first goes through `authorize_caller`, so the bridge-auth invariant holds: a request
//! with a forwarder header, a foreign `Host` or a foreign `Origin` needs a paired token.
//! Start, Keep and Throw away are not routes here (SPEC 3.7, 3.8): nothing in this file reaches
//! the Keep or Throw away code.

use serde_json::{json, Value};

use super::{Ctx, RouteResult};
use crate::audit::{Kind, Who};
use crate::mobile_pairing::Caller;
use crate::night_shift::model::{NightRun, RunState};
use crate::night_shift::runner;
use crate::night_shift::view::{self, NightView};

/// What the route touches. The live one is [`Live`]; tests use a fake that records the calls.
pub(crate) trait NightPort {
    /// The current or last run, from the app's own store. `None` when Night Shift never ran.
    fn current(&self) -> Option<NightRun>;
    /// The full view of a run. The route redacts it before it leaves.
    fn view(&self, run: &NightRun) -> NightView;
    /// Ask the supervisor to stop the run at its next tick.
    fn request_stop(&self, run_id: &str);
    /// One audit line (kind `night_shift`). The detail never carries task text.
    fn audit(&self, who: Who, summary: &str, detail: Value);
}

/// Who is on the other end, as the route sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Actor {
    /// A program on this computer (no pairing, no forwarder).
    Local,
    /// A paired phone, by its device id.
    Phone(String),
}

impl Actor {
    fn who(&self) -> Who {
        match self {
            Actor::Local => Who::Local,
            Actor::Phone(id) => Who::Device(Some(id.clone())),
        }
    }
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    handle_request(cx.method, cx.segments, cx.headers, cx.peer_loopback, cx.session, &Live)
}

/// `None` when the request is not `GET /night` or `POST /night/stop`.
pub(crate) fn handle_request(
    method: &str,
    raw_segments: &[&str],
    headers: &str,
    peer_loopback: bool,
    session: Option<&str>,
    port: &dyn NightPort,
) -> Option<RouteResult> {
    let segments = crate::mobile_pairing::normalize_bridge_segments(raw_segments);
    let stop = match (method, segments) {
        ("GET", ["night"]) => false,
        ("POST", ["night", "stop"]) => true,
        _ => return None,
    };
    let actor = match crate::mobile_pairing::authorize_caller(headers, peer_loopback, raw_segments, method) {
        Ok(Caller::Local) => Actor::Local,
        Ok(Caller::Device { device_id, .. }) => Actor::Phone(device_id),
        Ok(Caller::Public | Caller::Brain { .. }) => return Some(person_only()),
        Err(refusal) => return Some(Err(refusal)),
    };
    if session.is_some() {
        return Some(person_only());
    }
    Some(if stop { stop_run(&actor, port) } else { read_run(port) })
}

fn read_run(port: &dyn NightPort) -> RouteResult {
    let Some(run) = port.current() else { return Err(no_night()) };
    Ok((200, json_of(&port.view(&run).redacted_for_bridge())))
}

fn stop_run(actor: &Actor, port: &dyn NightPort) -> RouteResult {
    let Some(run) = port.current() else { return Err(no_night()) };
    if !matches!(run.state, RunState::Scheduled { .. } | RunState::Running) {
        return Err((
            409,
            json!({
                "error": "night_not_running",
                "code": "NOT_RUNNING",
                "message": "Night Shift is not running, so there is nothing to stop.",
            }),
        ));
    }
    port.request_stop(&run.id);
    port.audit(actor.who(), "Stopped Night Shift", json!({ "run": run.id }));
    Ok((200, json_of(&port.view(&run).redacted_for_bridge())))
}

fn json_of(view: &NightView) -> Value {
    serde_json::to_value(view).unwrap_or(Value::Null)
}

fn no_night() -> (u16, Value) {
    (404, json!({ "error": "no_night", "code": "NO_NIGHT", "message": "Night Shift has not run yet." }))
}

fn person_only() -> RouteResult {
    Err((
        403,
        json!({
            "error": "night_person_only",
            "code": "PERSON_ONLY",
            "hint": "Night Shift is for the person. An agent cannot read or stop it.",
        }),
    ))
}

/// The live port: the app's store, the redaction context of this process, the supervisor and the audit log.
struct Live;

impl NightPort for Live {
    #[cfg(not(test))]
    fn current(&self) -> Option<NightRun> {
        crate::night_shift::store::Store::live().load()
    }

    #[cfg(test)]
    fn current(&self) -> Option<NightRun> {
        None
    }

    fn view(&self, run: &NightRun) -> NightView {
        let offset = chrono::Local::now().offset().local_minus_utc() / 60;
        view::view_of(run, &crate::redact::known::context(), offset)
    }

    fn request_stop(&self, run_id: &str) {
        runner::request_stop(run_id);
    }

    fn audit(&self, who: Who, summary: &str, detail: Value) {
        crate::audit::record(Kind::NightShift, who, summary, detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobile_pairing::{init_pairing_store, pairing_store, Cap, Caps, Need, PairRequestBody, route_need};
    use crate::night_shift::model::{Event, RunState};
    use crate::night_shift::view::morning_fixture;
    use crate::redact::RedactContext;
    use std::cell::RefCell;
    use std::path::Path;

    /// Records what the route asked for, and answers from a fixed run.
    struct Fake {
        run: Option<NightRun>,
        stops: RefCell<Vec<String>>,
        audits: RefCell<Vec<(Who, String, Value)>>,
    }

    impl Fake {
        fn with(run: Option<NightRun>) -> Fake {
            Fake { run, stops: RefCell::new(Vec::new()), audits: RefCell::new(Vec::new()) }
        }
    }

    impl NightPort for Fake {
        fn current(&self) -> Option<NightRun> {
            self.run.clone()
        }
        fn view(&self, run: &NightRun) -> NightView {
            view::view_of(run, &RedactContext::default(), 0)
        }
        fn request_stop(&self, run_id: &str) {
            self.stops.borrow_mut().push(run_id.to_string());
        }
        fn audit(&self, who: Who, summary: &str, detail: Value) {
            self.audits.borrow_mut().push((who, summary.to_string(), detail));
        }
    }

    const LOCAL: &str = "Host: 127.0.0.1:17321\r\n";
    const FORWARDED: &str = "Host: 127.0.0.1:17321\r\nX-Forwarded-For: 203.0.113.9\r\n";

    fn with_state(state: RunState) -> NightRun {
        let mut run = morning_fixture();
        run.state = state;
        run
    }

    fn running() -> NightRun {
        with_state(RunState::Running)
    }

    fn call(method: &str, segments: &[&str], headers: &str, loopback: bool, session: Option<&str>, port: &dyn NightPort) -> (u16, Value) {
        match handle_request(method, segments, headers, loopback, session, port).expect("a night route") {
            Ok(ok) => ok,
            Err(err) => err,
        }
    }

    fn refused(result: (u16, Value), status: u16, code: &str) {
        assert_eq!(result.0, status, "{}", result.1);
        assert_eq!(result.1["code"], code, "{}", result.1);
    }

    /// A paired phone with `caps`, paired the same way the attention route tests pair one.
    fn pair(caps: Caps) -> (String, String) {
        if pairing_store().is_none() {
            let dir = std::env::temp_dir().join(format!("pm-night-bridge-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let _ = init_pairing_store(dir.join("pairing.json"));
        }
        let store = pairing_store().unwrap();
        let mut guard = store.lock();
        let s = guard.create_pairing_session_with_caps("http://127.0.0.1:1".into(), caps);
        let paired = guard
            .pair_device(PairRequestBody { pairing_code: s.pairing_code, device_name: "Test phone".into(), device_public_key: s.server_public_key })
            .unwrap();
        (paired.device_id, paired.device_token)
    }

    /// Headers of a request from a phone over the tunnel: a remote peer with its bearer token.
    fn phone_headers(token: &str) -> String {
        format!("Host: pm.example.com\r\nAuthorization: Bearer {token}\r\n")
    }

    #[test]
    fn the_read_is_the_redacted_view_with_no_path_pane_id_or_sentence() {
        let fake = Fake::with(Some(running()));
        let (status, body) = call("GET", &["night"], LOCAL, true, None, &fake);
        assert_eq!(status, 200, "{body}");
        let text = body.to_string();
        // A JSON string escapes every backslash, so any path separator shows up here.
        assert!(!text.contains('\\'), "a backslash leaked: {text}");
        for leak in ["/home/", "C:/", "pane-42", "pane-43", "Fixed the rounding", "<script>"] {
            assert!(!text.contains(leak), "{leak} leaked: {text}");
        }
        assert!(body["tasks"][0]["project"].is_null());
        assert!(body["tasks"][0]["paneId"].is_null());
        assert!(body["tasks"][0]["summaries"].as_array().unwrap().is_empty());
        assert!(body["line"].is_string());
        assert_eq!(body["id"], "ns-0123456789ab");
        // The bridge prefix is the same route.
        assert_eq!(call("GET", &["bridge", "night"], LOCAL, true, None, &fake).0, 200);
    }

    #[test]
    fn no_night_yet_is_404_for_read_and_stop_and_nothing_is_stopped() {
        let fake = Fake::with(None);
        refused(call("GET", &["night"], LOCAL, true, None, &fake), 404, "NO_NIGHT");
        refused(call("POST", &["night", "stop"], LOCAL, true, None, &fake), 404, "NO_NIGHT");
        assert!(fake.stops.borrow().is_empty());
        assert!(fake.audits.borrow().is_empty());
    }

    #[test]
    fn an_agent_mcp_session_is_refused_for_read_and_stop() {
        let fake = Fake::with(Some(running()));
        refused(call("GET", &["night"], LOCAL, true, Some("mcp-session-1"), &fake), 403, "PERSON_ONLY");
        refused(call("POST", &["night", "stop"], LOCAL, true, Some("mcp-session-1"), &fake), 403, "PERSON_ONLY");
        assert!(fake.stops.borrow().is_empty());
        assert!(fake.audits.borrow().is_empty());
    }

    #[test]
    fn a_forwarder_or_a_foreign_host_or_origin_on_loopback_is_not_trusted() {
        for headers in [FORWARDED, "Host: evil.example.com\r\n", "Host: 127.0.0.1:17321\r\nOrigin: https://evil.example.com\r\n"] {
            let fake = Fake::with(Some(running()));
            // No bearer token: the shared auth refuses before the route is reached.
            assert_eq!(call("GET", &["night"], headers, true, None, &fake).0, 401, "{headers}");
            assert_eq!(call("POST", &["night", "stop"], headers, true, None, &fake).0, 401, "{headers}");
            assert!(fake.stops.borrow().is_empty());
        }
    }

    #[test]
    fn a_plain_loopback_program_reads_and_stops_and_the_stop_is_audited_as_local() {
        let fake = Fake::with(Some(running()));
        let (status, body) = call("POST", &["night", "stop"], LOCAL, true, None, &fake);
        assert_eq!(status, 200, "{body}");
        assert_eq!(*fake.stops.borrow(), vec!["ns-0123456789ab".to_string()]);
        let audits = fake.audits.borrow();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].0, Who::Local);
        assert_eq!(audits[0].1, "Stopped Night Shift");
        assert_eq!(audits[0].2, json!({ "run": "ns-0123456789ab" }));
    }

    #[test]
    fn a_stop_when_the_night_is_over_is_refused_and_changes_nothing() {
        for state in [RunState::Done, RunState::Stopped] {
            let fake = Fake::with(Some(with_state(state)));
            refused(call("POST", &["night", "stop"], LOCAL, true, None, &fake), 409, "NOT_RUNNING");
            assert!(fake.stops.borrow().is_empty());
            assert!(fake.audits.borrow().is_empty());
        }
    }

    #[test]
    fn a_scheduled_night_can_be_stopped_before_it_starts() {
        let fake = Fake::with(Some(with_state(RunState::Scheduled { start_at_ms: 9_000_000 })));
        assert_eq!(call("POST", &["night", "stop"], LOCAL, true, None, &fake).0, 200);
        assert_eq!(fake.stops.borrow().len(), 1);
    }

    #[test]
    fn a_paired_phone_needs_watch_to_read_and_control_to_stop() {
        assert_eq!(route_need("GET", &["night"]), Need::Cap(Cap::Watch));
        assert_eq!(route_need("POST", &["night", "stop"]), Need::Cap(Cap::Control));
        assert!(!Caps::none().satisfies(route_need("GET", &["night"])));
        assert!(Caps::watch_and_answer().satisfies(route_need("GET", &["night"])));
        assert!(!Caps::watch_and_answer().satisfies(route_need("POST", &["night", "stop"])));
        assert!(Caps::all().satisfies(route_need("POST", &["night", "stop"])));
    }

    #[test]
    fn a_paired_phone_without_control_cannot_stop_and_one_with_control_stops_under_its_own_id() {
        let fake = Fake::with(Some(running()));

        let (_, watch_token) = pair(Caps::watch_and_answer());
        let headers = phone_headers(&watch_token);
        assert_eq!(call("GET", &["night"], &headers, false, None, &fake).0, 200);
        refused(call("POST", &["night", "stop"], &headers, false, None, &fake), 403, "DEVICE_NOT_ALLOWED");
        assert!(fake.stops.borrow().is_empty());

        let (control_id, control_token) = pair(Caps::all());
        let headers = phone_headers(&control_token);
        assert_eq!(call("POST", &["night", "stop"], &headers, false, None, &fake).0, 200);
        assert_eq!(*fake.stops.borrow(), vec!["ns-0123456789ab".to_string()]);
        let audits = fake.audits.borrow();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].0, Who::Device(Some(control_id)));
        // No task text, no path: only the run id.
        assert_eq!(audits[0].2, json!({ "run": "ns-0123456789ab" }));
    }

    #[test]
    fn other_routes_are_left_alone() {
        let fake = Fake::with(Some(running()));
        for (method, segments) in [
            ("POST", &["night"][..]),
            ("GET", &["night", "stop"][..]),
            ("GET", &["nights"][..]),
            ("POST", &["night", "start"][..]),
            ("DELETE", &["night"][..]),
        ] {
            assert!(handle_request(method, segments, LOCAL, true, None, &fake).is_none(), "{method} {segments:?}");
        }
        assert!(fake.stops.borrow().is_empty());
    }

    #[test]
    fn no_bridge_or_tool_file_names_the_keep_or_throw_away_code() {
        let forbidden = concat!("night_shift::", "review");
        for root in [concat!(env!("CARGO_MANIFEST_DIR"), "/src/bridge"), concat!(env!("CARGO_MANIFEST_DIR"), "/src/tool_registry")] {
            scan(Path::new(root), forbidden);
        }
    }

    fn scan(dir: &Path, needle: &str) {
        for entry in std::fs::read_dir(dir).expect("a readable source folder") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                scan(&path, needle);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).expect("a readable source file");
                assert!(!text.contains(needle), "{} names {needle}", path.display());
            }
        }
    }

    #[test]
    fn the_event_text_is_cut_on_the_bridge() {
        let mut run = running();
        run.events = vec![Event { at_ms: 3_600_000, kind: "note".into(), glyph: "~".into(), text: "x".repeat(300), task: None }];
        let fake = Fake::with(Some(run));
        let (_, body) = call("GET", &["night"], LOCAL, true, None, &fake);
        let text = body["events"][0]["text"].as_str().unwrap();
        assert!(text.chars().count() <= view::EVENT_BRIDGE_CHARS, "{text}");
    }
}
