//! Loader for the corpus format: asciicast v2 (<https://docs.asciinema.org/manual/asciicast/v2/>,
//! a JSON header line, then one JSON event per line) plus two small extensions that keep a cast
//! self-contained.
//!
//! Standard events, replayed on the timeline:
//! - `[t, "o", "<text>"]` output bytes the agent wrote at `t` seconds.
//! - `[t, "i", "<text>"]` input the app sent (kept as a record; the replay never re-sends it).
//! - `[t, "r", "<cols>x<rows>"]` the PTY was resized.
//! - `[t, "m", "<text>"]` a marker. Markers whose text starts with `edit:` model a file the agent
//!   changed in the workspace (`edit:<path>`); the receipt replay reads them.
//!
//! Extensions (ignored by other asciicast players, which skip unknown event codes):
//! - header `pm`: provenance and the starting state (`agent`, `agent_version`, `bug`, `source`,
//!   `initial_status`).
//! - `[delay, "ro", {"on": "...", "data": "..."}]` a reactive output: when the code under test
//!   writes `on` to the pane (and, when set, `enter` and `while_screen_contains` match), `data` is
//!   written `delay` seconds later. This is what lets a recorded screen sequence answer a typed
//!   Esc the way the real agent did, without a process. Each reaction fires once.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Output(Vec<u8>),
    Input(String),
    Resize { cols: u16, rows: u16 },
    Marker(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub at: Duration,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reaction {
    pub delay: Duration,
    /// Exact text the code under test must write.
    pub on: String,
    /// When set, the write must (not) press Enter.
    pub enter: Option<bool>,
    /// When set, the reaction only fires while the screen shows this text.
    pub while_screen_contains: Option<String>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub agent: String,
    pub agent_version: String,
    pub bug: String,
    pub source: Vec<String>,
    /// `running` (a pane right after spawn, the default) or `idle`.
    pub initial_status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub name: String,
    pub cols: u16,
    pub rows: u16,
    pub meta: Meta,
    pub events: Vec<Event>,
    pub reactions: Vec<Reaction>,
}

fn seconds(value: &Value, line: usize) -> Result<Duration, String> {
    let secs = value.as_f64().ok_or_else(|| format!("line {line}: time is not a number"))?;
    if !(0.0..=86_400.0).contains(&secs) {
        return Err(format!("line {line}: time {secs} out of range"));
    }
    Ok(Duration::from_micros((secs * 1_000_000.0).round() as u64))
}

impl Cast {
    pub fn parse(name: &str, text: &str) -> Result<Cast, String> {
        let mut lines = text.lines().enumerate().filter(|(_, line)| !line.trim().is_empty());
        let (_, head) = lines.next().ok_or("empty cast")?;
        let header: Value = serde_json::from_str(head).map_err(|e| format!("header: {e}"))?;
        if header["version"] != 2 {
            return Err(format!("header: version must be 2, got {}", header["version"]));
        }
        let dim = |key: &str| -> Result<u16, String> {
            header[key].as_u64().filter(|n| *n > 0 && *n < 1000).map(|n| n as u16).ok_or(format!("header: bad {key}"))
        };
        let pm = &header["pm"];
        let text_of = |key: &str| pm[key].as_str().unwrap_or_default().to_string();
        let meta = Meta {
            agent: text_of("agent"),
            agent_version: text_of("agent_version"),
            bug: text_of("bug"),
            source: pm["source"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            initial_status: pm["initial_status"].as_str().unwrap_or("running").to_string(),
        };
        let mut events = Vec::new();
        let mut reactions = Vec::new();
        for (index, line) in lines {
            let n = index + 1;
            let event: Value = serde_json::from_str(line).map_err(|e| format!("line {n}: {e}"))?;
            let parts = event.as_array().filter(|a| a.len() == 3).ok_or_else(|| format!("line {n}: an event is [time, code, data]"))?;
            let at = seconds(&parts[0], n)?;
            let code = parts[1].as_str().ok_or_else(|| format!("line {n}: code is not a string"))?;
            let data = &parts[2];
            let str_data = || data.as_str().ok_or_else(|| format!("line {n}: data is not a string"));
            match code {
                "o" => events.push(Event { at, kind: Kind::Output(str_data()?.as_bytes().to_vec()) }),
                "i" => events.push(Event { at, kind: Kind::Input(str_data()?.to_string()) }),
                "m" => events.push(Event { at, kind: Kind::Marker(str_data()?.to_string()) }),
                "r" => {
                    let (cols, rows) = str_data()?
                        .split_once('x')
                        .and_then(|(c, r)| Some((c.parse().ok()?, r.parse().ok()?)))
                        .ok_or_else(|| format!("line {n}: resize is \"<cols>x<rows>\""))?;
                    events.push(Event { at, kind: Kind::Resize { cols, rows } });
                }
                "ro" => reactions.push(Reaction {
                    delay: at,
                    on: data["on"].as_str().ok_or_else(|| format!("line {n}: reaction needs \"on\""))?.to_string(),
                    enter: data["enter"].as_bool(),
                    while_screen_contains: data["while_screen_contains"].as_str().map(str::to_string),
                    data: data["data"].as_str().ok_or_else(|| format!("line {n}: reaction needs \"data\""))?.as_bytes().to_vec(),
                }),
                // asciicast players skip codes they do not know; so do we.
                _ => {}
            }
        }
        if events.windows(2).any(|pair| pair[1].at < pair[0].at) {
            return Err("events are not in time order".to_string());
        }
        Ok(Cast { name: name.to_string(), cols: dim("width")?, rows: dim("height")?, meta, events, reactions })
    }

    /// `tests/corpus/<relative>` of this crate, e.g. `codex/0.159.3/update_menu_at_300ms.cast`.
    pub fn corpus(relative: &str) -> Cast {
        let path = corpus_dir().join(relative);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read cast {}: {e}", path.display()));
        Cast::parse(relative, &text).unwrap_or_else(|e| panic!("bad cast {relative}: {e}"))
    }

    /// When the last standard event happens.
    pub fn end(&self) -> Duration {
        self.events.last().map_or(Duration::ZERO, |e| e.at)
    }

    /// Markers `edit:<path>` with their times: files the agent changed in the workspace.
    pub fn edits(&self) -> Vec<(Duration, String)> {
        self.events
            .iter()
            .filter_map(|e| match &e.kind {
                Kind::Marker(text) => text.strip_prefix("edit:").map(|path| (e.at, path.to_string())),
                _ => None,
            })
            .collect()
    }
}

pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("corpus")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        r#"{"version":2,"width":80,"height":24,"pm":{"agent":"codex","initial_status":"idle","source":["a.txt"]}}"#,
        "\n",
        r#"[0.5,"o","hello\r\n"]"#,
        "\n",
        r#"[1.25,"r","100x40"]"#,
        "\n",
        r#"[1.3,"m","edit:src/a.rs"]"#,
        "\n",
        r#"[2.0,"i","\u001b"]"#,
        "\n",
        r#"[0.1,"ro",{"on":"\u001b","enter":false,"data":"closed"}]"#,
        "\n",
        r#"[3.0,"x","unknown codes are skipped"]"#,
        "\n",
    );

    #[test]
    fn parses_events_markers_resizes_and_reactions() {
        let cast = Cast::parse("sample", SAMPLE).unwrap();
        assert_eq!((cast.cols, cast.rows), (80, 24));
        assert_eq!(cast.meta.agent, "codex");
        assert_eq!(cast.meta.initial_status, "idle");
        assert_eq!(cast.events.len(), 4);
        assert_eq!(cast.events[0], Event { at: Duration::from_millis(500), kind: Kind::Output(b"hello\r\n".to_vec()) });
        assert_eq!(cast.events[1].kind, Kind::Resize { cols: 100, rows: 40 });
        assert_eq!(cast.edits(), vec![(Duration::from_millis(1300), "src/a.rs".to_string())]);
        assert_eq!(cast.reactions.len(), 1);
        assert_eq!(cast.reactions[0].delay, Duration::from_millis(100));
        assert_eq!(cast.end(), Duration::from_secs(2));
    }

    #[test]
    fn refuses_what_is_not_a_v2_cast() {
        assert!(Cast::parse("x", "").is_err());
        assert!(Cast::parse("x", r#"{"version":3,"width":1,"height":1}"#).is_err());
        let head = r#"{"version":2,"width":80,"height":24}"#;
        let bad_order = format!("{head}\n[2.0,\"o\",\"a\"]\n[1.0,\"o\",\"b\"]\n");
        assert!(Cast::parse("x", &bad_order).unwrap_err().contains("time order"));
        let bad_event = format!("{head}\n[1.0,\"o\"]\n");
        assert!(Cast::parse("x", &bad_event).is_err());
    }
}
