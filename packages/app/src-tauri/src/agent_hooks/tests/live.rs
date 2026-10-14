//! Live shadow run harness (ignored; run by hand, see docs/specs/hook-lane/LIVE.md).
//!
//! `live_serve`: the real bridge gate + `/hooks/event` route on a loopback port, a pane token
//! minted by the real `prepare_spawn_in` (which writes Claude's `--settings` file or Codex's
//! `.codex/hooks.json` with the real writer and the frozen copy of the built MCP exe). It writes
//! `launch.json` (env + args for the agent) to `LIVE_DIR`, serves until `LIVE_DIR/stop` exists,
//! then writes every hook event the ledger stored to `hook-events.json`.
//!
//! The agent itself runs on a ConPTY outside the test (no window), recorded byte for byte with
//! timing to `LIVE_DIR/live.cast`.
//!
//! `live_side_by_side`: replays that recording through the app's own status pipeline
//! (`replay::pane::ReplayPane`: the PTY reader's classifier, busy marker and idle watcher) on the
//! recorded timing, merges the screen verdicts with the hook events by wall-clock time, and feeds
//! both into a `Shadow` recorder: `LIVE_DIR/side-by-side/<pane>.jsonl` plus `summary.json`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::super::*;
use crate::agent_hooks::model::HookEvent;
use crate::agent_hooks::shadow::Shadow;

fn live_dir() -> PathBuf {
    PathBuf::from(std::env::var("LIVE_DIR").expect("LIVE_DIR"))
}

#[test]
#[ignore = "live: run by hand with LIVE_DIR, LIVE_AGENT, LIVE_CWD, LIVE_MCP_EXE"]
fn live_serve() {
    let dir = live_dir();
    let agent = std::env::var("LIVE_AGENT").expect("LIVE_AGENT");
    let cwd = std::env::var("LIVE_CWD").expect("LIVE_CWD");
    let exe = std::env::var("LIVE_MCP_EXE").expect("LIVE_MCP_EXE");
    let addr = crate::bridge::spawn_gate_listener();
    let pane = format!("live-{agent}-{}", std::process::id());
    let (state, launch) = prepare_spawn_in(&agent, &pane, &cwd, &[], &dir.join("hooks"), Some(Path::new(&exe)), Some(addr.to_string()));
    let env: serde_json::Map<String, Value> = launch.env.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    std::fs::write(
        dir.join("launch.json"),
        serde_json::to_string_pretty(&json!({ "state": state, "pane": pane, "bridge": addr.to_string(), "env": env, "extra_args": launch.extra_args })).unwrap(),
    )
    .unwrap();
    eprintln!("serving {addr} for pane {pane}: {state:?}");
    let deadline = std::time::Instant::now() + Duration::from_secs(20 * 60);
    while !dir.join("stop").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(300));
    }
    let events = ledger::recent(&pane);
    std::fs::write(dir.join("hook-events.json"), serde_json::to_string_pretty(&events).unwrap()).unwrap();
    std::fs::write(dir.join("facts.json"), serde_json::to_string_pretty(&ledger::facts(&pane)).unwrap()).unwrap();
    eprintln!("{} hook events, rejections {:?}", events.len(), rejection_counts());
    revoke(&pane);
}

#[test]
#[ignore = "live: run by hand after live_serve, with LIVE_DIR"]
fn live_side_by_side() {
    let dir = live_dir();
    let text = std::fs::read_to_string(dir.join("live.cast")).unwrap();
    let header: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    let start_ms = header["pm"]["start_ms"].as_u64().unwrap();
    let cast = crate::replay::cast::Cast::parse("live", &text).unwrap();
    let mut screen = crate::replay::pane::ReplayPane::new(cast);
    screen.run_to_end(Duration::from_secs(20));
    let events: Vec<HookEvent> = serde_json::from_str(&std::fs::read_to_string(dir.join("hook-events.json")).unwrap()).unwrap();
    let pane = events.first().map(|e| e.pane_id.clone()).expect("at least one hook event");

    enum Item<'a> {
        Screen(&'static str),
        Hook(&'a HookEvent),
    }
    let mut items: Vec<(u64, Item)> = screen
        .transitions
        .iter()
        .map(|(at, status)| (start_ms + at.as_millis() as u64, Item::Screen(status.as_str())))
        .collect();
    items.extend(events.iter().map(|e| (e.at_ms, Item::Hook(e))));
    items.sort_by_key(|(t, _)| *t);

    let out = dir.join("side-by-side");
    let _ = std::fs::remove_dir_all(&out);
    let mut shadow = Shadow::new(Some(out.clone()));
    let mut table = String::from("  t (s)  screen            hook\n");
    for (t, item) in &items {
        let secs = (*t as f64 - start_ms as f64) / 1000.0;
        match item {
            Item::Screen(status) => {
                shadow.observe_screen(&pane, status, *t);
                table.push_str(&format!("{secs:7.2}  {status:<16}\n"));
            }
            Item::Hook(event) => {
                shadow.observe_hook(event, event.applied, *t);
                let note = if event.applied { "" } else { " (recorded, not applied)" };
                table.push_str(&format!("{secs:7.2}  {:<16}  {}{note}\n", "", event.vendor_event));
            }
        }
    }
    let end = items.last().map(|(t, _)| *t).unwrap_or(start_ms) + 60_000;
    shadow.sweep(end);
    let tallies: Vec<Value> = shadow
        .tallies()
        .into_iter()
        .map(|(agent, check, t)| json!({ "agent": agent, "check": check, "agree": t.agree, "screen_only": t.screen_only, "hook_only": t.hook_only }))
        .collect();
    std::fs::write(dir.join("summary.json"), serde_json::to_string_pretty(&json!({ "pane": pane, "tallies": tallies })).unwrap()).unwrap();
    std::fs::write(dir.join("side-by-side.txt"), &table).unwrap();
    eprintln!("{table}\n{}", serde_json::to_string_pretty(&tallies).unwrap());
}
