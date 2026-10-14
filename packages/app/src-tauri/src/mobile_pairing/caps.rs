//! What a paired device may do (S2 device capabilities).
//!
//! A pairing holds a set of [`Cap`]s. Every bridge route is put in one class by [`route_need`],
//! and `authorize_bridge_request` refuses (403 `not_allowed`) a device whose caps do not cover it.
//! The event streams (`/events`, `/events/ws`, `/events/poll`) are open to any paired device, but
//! `event: terminal` payloads (raw pane bytes) only reach devices that hold `watch`
//! ([`may_receive`]).
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
        // checks the answering device's caps itself).
        (_, ["push", ..]) => Need::Device,

        ("GET", ["questions"]) => Need::AnyOf(Watch, Answer),
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
        (_, ["panes", "wait", ..]) => Need::Cap(Watch),

        ("GET", _) => Need::Cap(Watch),
        _ => Need::Cap(Control),
    }
}

/// True for an `event: terminal` payload (live pane bytes).
pub fn is_terminal_payload(payload: &str) -> bool {
    payload.starts_with("event: terminal\n")
}

/// Whether a device with `caps` gets this event-stream payload.
pub fn may_receive(caps: Caps, payload: &str) -> bool {
    caps.has(Cap::Watch) || !is_terminal_payload(payload)
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
    fn terminal_bytes_need_watch_and_nothing_else_is_filtered() {
        let term = "event: terminal\ndata: {\"pane_id\":\"p\",\"b64\":\"eA==\"}\n\n";
        assert!(!may_receive(Caps::none().with(Cap::Answer, true), term));
        assert!(may_receive(Caps::none().with(Cap::Watch, true), term));
        assert!(may_receive(Caps::none(), "event: panes\ndata: []\n\n"));
        assert!(may_receive(Caps::none(), "event: pane-status\ndata: {}\n\n"));
    }
}
