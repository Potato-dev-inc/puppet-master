//! What a paired device may do (S2 device capabilities).
//!
//! A pairing holds a set of [`Cap`]s. Every bridge route is put in one class by [`route_need`],
//! and `authorize_bridge_request` refuses (403 `not_allowed`) a device whose caps do not cover it.
//! The event streams (`/events`, `/events/ws`, `/events/poll`) are open to any paired device, but
//! what a device without `watch` gets is cut down to what it may see ([`project`]): no terminal
//! bytes, and no task, reply, screen rows or transcript text inside the other events.
//!
//! The local desktop app and its tools are not devices: they are never limited by caps.

use serde::{Deserialize, Serialize};

/// One thing a paired device may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    /// See terminals: pane screens, buffers, transcripts and the live terminal bytes.
    Watch,
    /// Answer approval and question cards.
    Answer,
    /// Type into a live terminal (pane input and keys).
    Type,
    /// Start and close agents, run shell commands, change settings: everything else that acts.
    Control,
}

impl Cap {
    pub const ALL: [Cap; 4] = [Cap::Watch, Cap::Answer, Cap::Type, Cap::Control];

    fn bit(self) -> u8 {
        match self {
            Cap::Watch => 1,
            Cap::Answer => 2,
            Cap::Type => 4,
            Cap::Control => 8,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Cap::Watch => "watch",
            Cap::Answer => "answer",
            Cap::Type => "type",
            Cap::Control => "control",
        }
    }
}

/// A set of [`Cap`]s. Serialized as a list of names (`["watch","answer"]`); unknown names in a
/// stored list are ignored, so a newer app's caps never widen what an older one grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caps(u8);

impl Caps {
    /// Today's behaviour, and what every pairing made before caps existed migrates to.
    pub const fn all() -> Self {
        Caps(1 | 2 | 4 | 8)
    }

    /// "Watch and answer only" at pairing time.
    pub const fn watch_and_answer() -> Self {
        Caps(1 | 2)
    }

    pub const fn none() -> Self {
        Caps(0)
    }

    pub fn has(self, cap: Cap) -> bool {
        self.0 & cap.bit() != 0
    }

    pub fn with(self, cap: Cap, on: bool) -> Self {
        if on {
            Caps(self.0 | cap.bit())
        } else {
            Caps(self.0 & !cap.bit())
        }
    }

    pub fn list(self) -> Vec<Cap> {
        Cap::ALL.into_iter().filter(|cap| self.has(*cap)).collect()
    }

    /// Parse names from the UI or a route body. Unknown names are an error (the caller typed it).
    pub fn parse_names<S: AsRef<str>>(names: &[S]) -> Result<Self, String> {
        let mut caps = Caps::none();
        for name in names {
            let cap = match name.as_ref().trim() {
                "watch" => Cap::Watch,
                "answer" => Cap::Answer,
                "type" => Cap::Type,
                "control" => Cap::Control,
                other => return Err(format!("unknown capability {other:?}")),
            };
            caps = caps.with(cap, true);
        }
        Ok(caps)
    }

    pub fn satisfies(self, need: Need) -> bool {
        match need {
            Need::Device => true,
            Need::Cap(cap) => self.has(cap),
            Need::AnyOf(a, b) => self.has(a) || self.has(b),
        }
    }
}

impl Serialize for Caps {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.list().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Caps {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(deserializer)?;
        Ok(names.iter().fold(Caps::none(), |acc, name| {
            match Caps::parse_names(&[name.as_str()]) {
                Ok(one) => Caps(acc.0 | one.0),
                Err(_) => acc,
            }
        }))
    }
}

/// What a route asks of a paired device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Any paired device (the pane list, the event streams, its own push subscription).
    Device,
    Cap(Cap),
    /// Either cap is enough (the question list serves both watching and answering).
    AnyOf(Cap, Cap),
}

/// The class of a bridge route. `segments` are already normalized (no leading `bridge`).
/// Read routes need `watch` and anything that acts needs `control`, unless listed here: an
/// unknown or new route fails closed into one of those two.
pub fn route_need(method: &str, segments: &[&str]) -> Need {
    use Cap::*;
    match (method, segments) {
        // The bridge banner, the pane list, public settings and agent names: already in the first
        // frames of every event stream.
        ("GET", [] | ["panes"] | ["settings"] | ["agents", "types"] | ["pair", "me"]) => Need::Device,
        // The event streams themselves; terminal bytes inside them are filtered by `watch`.
        ("GET", ["events"] | ["events", "ws"] | ["events", "poll"]) => Need::Device,
        // This device's own notification subscription and presence (`/push/act` is public and
        // checks the answering device's caps itself). Listed exactly: a push route added later
        // falls through to watch/control instead of being open to any device.
        ("GET", ["push", "key"]) | ("POST" | "DELETE", ["push", "subscribe"]) | ("POST", ["push", "presence" | "act"]) => {
            Need::Device
        }

        ("GET", ["questions"]) => Need::AnyOf(Watch, Answer),
        // The attention list (what needs the person): sentences, not terminal bytes.
        ("GET", ["attention"]) => Need::AnyOf(Watch, Answer),
        ("POST", ["questions", _, "answer" | "dismiss"]) => Need::Cap(Answer),
        ("POST", ["agents", "answer"]) => Need::Cap(Answer),
        ("POST", [_, _, "opencode", "permissions", _, "reply"]) => Need::Cap(Answer),
        ("POST", [_, _, "opencode", "question", "reply"]) => Need::Cap(Answer),

        ("POST", ["panes", _, "input" | "key" | "resize"]) => Need::Cap(Type),

        // Team cards (`team::phone`): dismissing is an answer; pull, queue, apply, discard act.
        ("POST", ["team", "cards", _, "dismiss"]) => Need::AnyOf(Answer, Control),

        // Holding a pane at this phone's size while it is on screen is part of watching.
        ("POST", ["panes", _, "viewport"] | ["orchestrator", "viewport"]) => Need::Cap(Watch),
        // Long-poll waits are reads sent as POST.
        ("POST", ["agents", "wait"] | ["operations", _, "wait"]) => Need::Cap(Watch),
        // Exactly the four wait routes (`bridge/routes/wait.rs`): anything else under
        // `/panes/wait` is a pane route on a pane named `wait` and must not pass as a read.
        ("POST", ["panes", "wait"] | ["panes", "wait", "model" | "task" | "worker"]) => Need::Cap(Watch),

        ("GET", _) => Need::Cap(Watch),
        _ => Need::Cap(Control),
    }
}

/// True for an `event: terminal` payload (live pane bytes).
pub fn is_terminal_payload(payload: &str) -> bool {
    payload.starts_with("event: terminal
")
}

/// Whether a device with `caps` gets this event-stream payload in some form.
pub fn may_receive(caps: Caps, payload: &str) -> bool {
    project(caps, payload).is_some()
}

/// Events a device without `watch` gets unchanged: structure and status, no pane text.
const NO_WATCH_AS_IS: &[&str] = &[
    "panes",                     // the pane list (also `GET /panes`, open to any device)
    "settings",                  // public settings (also `GET /settings`)
    "pane-status",               // an enum
    "pane-resize",               // geometry
    "pane-activity",             // the one-line status phrase the pane list shows
    "pane-preview",              // dev-server URLs
    "questions",                 // a revision number; the cards come from `GET /questions`
    "opencode-worker",           // status, counts and permission ids
    "orchestrator-viewport",     // geometry
    "agent_definitions_changed", // agent names
    "turn-summary",              // counts, file paths and the approval questions (answer cards)
];

/// Top-level `event: operation` fields a device without `watch` gets: ids, status and the card.
/// Everything else (`task`, `result`, `output_baseline`, `result_capture`, checks, ...) is pane
/// or transcript content, and a field added later stays hidden until it is listed here.
const OPERATION_NO_WATCH_FIELDS: &[&str] = &[
    "operation_id",
    "runtime_id",
    "project_path",
    "agent_type",
    "pane_id",
    "pane_created",
    "task_id",
    "status",
    "revision",
    "created_at_ms",
    "updated_at_ms",
    "observed_at_ms",
    "pane_state",
    "source",
    "stage",
    "progress_pct",
    "timeout_ms",
    "read_only",
    "placement",
    "agent_run_id",
    "turn_index",
    "verified",
    "started_at_ms",
    "finished_at_ms",
    "acceptance_status",
];

/// `required_action` keys that carry screen text rather than the card itself.
const ACTION_SCREEN_KEYS: &[&str] = &["screen", "raw_prompt", "last_screen", "context"];

/// What a device with `caps` gets for one event-stream payload: `None` leaves it out, otherwise
/// the payload as is or cut down. With `watch` everything goes through. Without it:
/// - `event: terminal` (raw pane bytes) and `event: chat` (the orchestrator transcript) are left out;
/// - `event: operation` keeps ids, status, the error's code and message, and `required_action`
///   without its screen rows ([`OPERATION_NO_WATCH_FIELDS`]);
/// - `event: loop-signal` loses its `evidence_lines` (screen lines);
/// - the events in [`NO_WATCH_AS_IS`] pass unchanged;
/// - any other event (a new one, or one this list does not know) is left out: fail closed.
pub fn project(caps: Caps, payload: &str) -> Option<std::borrow::Cow<'_, str>> {
    use std::borrow::Cow;
    if caps.has(Cap::Watch) {
        return Some(Cow::Borrowed(payload));
    }
    let rest = payload.strip_prefix("event: ")?;
    let (name, data) = rest.split_once("
data: ")?;
    if NO_WATCH_AS_IS.contains(&name) {
        return Some(Cow::Borrowed(payload));
    }
    let trim: fn(&mut serde_json::Value) = match name {
        "operation" => trim_operation,
        "loop-signal" => |value| {
            if let Some(lines) = value.get_mut("signal").and_then(|s| s.get_mut("evidence_lines")) {
                *lines = serde_json::json!([]);
            }
        },
        _ => return None,
    };
    let body = data.strip_suffix("

").unwrap_or(data);
    let mut value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    trim(&mut value);
    let json = serde_json::to_string(&value).ok()?;
    Some(Cow::Owned(format!("event: {name}
data: {json}

")))
}

/// The no-`watch` view of an operation snapshot (see [`project`]).
fn trim_operation(value: &mut serde_json::Value) {
    use serde_json::{Map, Value};
    let Some(full) = value.as_object() else {
        *value = Value::Null;
        return;
    };
    let mut out: Map<String, Value> =
        full.iter().filter(|(k, _)| OPERATION_NO_WATCH_FIELDS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    if let Some(Value::Object(action)) = full.get("required_action") {
        let action: Map<String, Value> =
            action.iter().filter(|(k, _)| !ACTION_SCREEN_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        out.insert("required_action".into(), Value::Object(action));
    } else if full.contains_key("required_action") {
        out.insert("required_action".into(), Value::Null);
    }
    if let Some(Value::Object(error)) = full.get("error") {
        let error: Map<String, Value> = error
            .iter()
            .filter(|(k, _)| matches!(k.as_str(), "code" | "message" | "recoverable" | "retry_after_ms"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.insert("error".into(), Value::Object(error));
    } else if full.contains_key("error") {
        out.insert("error".into(), Value::Null);
    }
    // Tells the client the content is held back, not missing.
    out.insert("content_hidden".into(), Value::String("needs_watch".into()));
    *value = Value::Object(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_round_trip_as_names_and_ignore_unknown_stored_names() {
        let caps = Caps::watch_and_answer();
        assert_eq!(serde_json::to_value(caps).unwrap(), serde_json::json!(["watch", "answer"]));
        let back: Caps = serde_json::from_value(serde_json::json!(["answer", "watch", "fly"])).unwrap();
        assert_eq!(back, caps);
        assert!(Caps::parse_names(&["watch", "fly"]).is_err());
        assert_eq!(Caps::parse_names(&["watch", "answer", "type", "control"]).unwrap(), Caps::all());
    }

    #[test]
    fn the_attention_list_is_open_to_a_phone_that_can_watch_or_answer() {
        assert_eq!(route_need("GET", &["attention"]), Need::AnyOf(Cap::Watch, Cap::Answer));
    }

    #[test]
    fn terminal_bytes_need_watch_and_nothing_else_is_filtered() {
        let term = "event: terminal\ndata: {\"pane_id\":\"p\",\"b64\":\"eA==\"}\n\n";
        assert!(!may_receive(Caps::none().with(Cap::Answer, true), term));
        assert!(may_receive(Caps::none().with(Cap::Watch, true), term));
        assert!(may_receive(Caps::none(), "event: panes\ndata: []\n\n"));
        assert!(may_receive(Caps::none(), "event: pane-status\ndata: {}\n\n"));
    }

    /// Review R12 2.2: `(_, ["panes", "wait", ..])` made every method on any `/panes/wait/...` path
    /// a read, so a watch-only device could `DELETE /panes/wait` or detach/retitle a pane named
    /// `wait`.
    #[test]
    fn only_the_four_wait_routes_are_reads_and_push_is_listed_exactly() {
        use Cap::*;
        for path in [&["panes", "wait"][..], &["panes", "wait", "model"], &["panes", "wait", "task"], &["panes", "wait", "worker"]] {
            assert_eq!(route_need("POST", path), Need::Cap(Watch), "{path:?}");
        }
        assert_eq!(route_need("DELETE", &["panes", "wait"]), Need::Cap(Control));
        assert_eq!(route_need("POST", &["panes", "wait", "detach"]), Need::Cap(Control));
        assert_eq!(route_need("POST", &["panes", "wait", "role"]), Need::Cap(Control));
        assert_eq!(route_need("POST", &["panes", "wait", "input"]), Need::Cap(Type));
        assert_eq!(route_need("GET", &["push", "key"]), Need::Device);
        assert_eq!(route_need("POST", &["push", "subscribe"]), Need::Device);
        assert_eq!(route_need("DELETE", &["push", "subscribe"]), Need::Device);
        assert_eq!(route_need("POST", &["push", "presence"]), Need::Device);
        assert_eq!(route_need("POST", &["push", "act"]), Need::Device);
        assert_eq!(route_need("POST", &["push", "broadcast"]), Need::Cap(Control), "a later push route fails closed");
    }

    fn data_of(frame: &str) -> serde_json::Value {
        let (_, data) = frame.split_once("\ndata: ").unwrap();
        serde_json::from_str(data.strip_suffix("\n\n").unwrap()).unwrap()
    }

    fn answer_only() -> Caps {
        Caps::none().with(Cap::Answer, true)
    }

    /// Review R12 2.1: an `event: operation` used to reach a device without `watch` whole: the task,
    /// the agent's reply, the screen baseline, 12 screen rows of a parked run and 40 lines of a
    /// mid-run exit.
    #[test]
    fn an_operation_event_reaches_a_device_without_watch_without_pane_content() {
        let snapshot = serde_json::json!({
            "operation_id": "op-1", "pane_id": "p1", "agent_type": "claude", "status": "waiting_input",
            "stage": "waiting_for_pane_readiness", "revision": 7,
            "task": "SECRET-TASK refactor the billing module",
            "result": "SECRET-REPLY all done",
            "output_baseline": "SECRET-BASELINE screen",
            "result_capture": {"text": "SECRET-CAPTURE"},
            "message_baseline_ids": ["m1"],
            "check_results": [{"output": "SECRET-CHECK"}],
            "required_action": {
                "kind": "screen_not_ready", "pane_id": "p1", "detail": "bring the agent to its input box",
                "screen": "SECRET-ROWS ❯ menu", "raw_prompt": {"text": "SECRET-RAW"}
            },
            "error": {"code": "AGENT_EXITED", "message": "the agent exited", "recoverable": false,
                      "context": {"last_screen": "SECRET-LAST-40"}},
            "some_future_field": "SECRET-FUTURE"
        });
        let frame = format!("event: operation\ndata: {snapshot}\n\n");
        let trimmed = project(answer_only(), &frame).expect("status still reaches the device").into_owned();
        assert!(!trimmed.contains("SECRET"), "pane content leaked to a device without watch: {trimmed}");
        let value = data_of(&trimmed);
        assert_eq!(value["operation_id"], "op-1");
        assert_eq!(value["status"], "waiting_input");
        assert_eq!(value["stage"], "waiting_for_pane_readiness");
        assert_eq!(value["required_action"]["kind"], "screen_not_ready");
        assert_eq!(value["required_action"]["detail"], "bring the agent to its input box");
        assert_eq!(value["error"]["code"], "AGENT_EXITED");
        assert_eq!(value["content_hidden"], "needs_watch");
        // With watch, the same frame goes out byte for byte.
        let full = Caps::none().with(Cap::Watch, true);
        assert_eq!(project(full, &frame).unwrap(), frame);
    }

    /// Every event name the bridge sends, for a device without `watch`: what passes, what is cut
    /// down, what is left out.
    #[test]
    fn every_event_kind_is_classified_for_a_device_without_watch() {
        let none = Caps::none();
        let as_is = [
            ("panes", r#"[]"#),
            ("settings", r#"{}"#),
            ("pane-status", r#"{"pane_id":"p","status":"idle"}"#),
            ("pane-resize", r#"{"pane_id":"p","cols":80,"rows":24}"#),
            ("pane-activity", r#"{"pane_id":"p","activity":null}"#),
            ("pane-preview", r#"{"pane_id":"p","urls":[]}"#),
            ("questions", r#"{"revision":3}"#),
            ("opencode-worker", r#"{"pane_id":"p","event":"updated"}"#),
            ("orchestrator-viewport", r#"{"width":1,"height":1,"active":true}"#),
            ("agent_definitions_changed", r#"{"added":[]}"#),
            ("turn-summary", r#"{"pane_id":"p","sentence":"Claude edited 1 file"}"#),
        ];
        for (name, data) in as_is {
            let frame = format!("event: {name}\ndata: {data}\n\n");
            assert_eq!(project(none, &frame).as_deref(), Some(frame.as_str()), "{name}");
        }
        for name in ["terminal", "chat", "a-future-event"] {
            let frame = format!("event: {name}\ndata: {{\"text\":\"SECRET\"}}\n\n");
            assert!(project(none, &frame).is_none(), "{name} must be left out");
            assert!(project(Caps::none().with(Cap::Watch, true), &frame).is_some(), "{name} with watch");
        }
        let signal = "event: loop-signal\ndata: {\"pane_id\":\"p\",\"signal\":{\"kind\":\"repeated_error\",\"summary\":\"the same error 4 times\",\"evidence_lines\":[\"SECRET error line\"]}}\n\n";
        let trimmed = project(none, signal).unwrap();
        assert!(!trimmed.contains("SECRET"), "{trimmed}");
        assert_eq!(data_of(&trimmed)["signal"]["summary"], "the same error 4 times");
        let cleared = "event: loop-signal\ndata: {\"pane_id\":\"p\",\"signal\":null}\n\n";
        assert_eq!(data_of(&project(none, cleared).unwrap())["signal"], serde_json::Value::Null);
    }
}
