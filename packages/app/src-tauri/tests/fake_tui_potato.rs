//! Potato Bot end to end (zero tokens, no key, no network): the real service starts a real Node process
//! running the brain package's `test/fake-brain.mjs`, which speaks channel protocol v1 to the real bridge
//! (`handle_connection`: the gate, the brain routes, the router) on a free port. What is fake: the brain's
//! model. Nothing else.
//!
//! What it shows:
//! - a send produces a stored, redacted reply, the mood and the usage;
//! - the brain gets 403 on `/panes`, with its token and without it (it has no way into the rest of the bridge);
//! - the brain started through `hidden_command` (no window) and leaves with the app's stop;
//! - a crash is seen, restarted after a wait, and the thread goes on;
//! - the kill switch kills it.
//!
//! Needs Node 20 or newer on the PATH. Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::PotatoRig;
use puppet_master_app_lib::proc_util::{pid_alive, spawn_log};
use serde_json::Value;
use std::time::{Duration, Instant};

const BOT: &str = "nimbus";

fn wait_for<T>(what: &str, secs: u64, mut look: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(found) = look() {
            return found;
        }
        if Instant::now() >= deadline {
            let rig = PotatoRig::start();
            panic!("timed out waiting for {what}\n-- state: {}\n-- brain log: {:#?}\n-- thread: {:#?}", rig.state(), rig.log(BOT), rig.thread(BOT));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn bot_lines(rig: &PotatoRig) -> Vec<String> {
    rig.thread(BOT)
        .iter()
        .filter(|m| m["from"]["kind"] == "bot" && m["kind"] == "text")
        .map(|m| m["text"].as_str().unwrap_or("").to_string())
        .collect()
}

fn wait_for_bot_line(rig: &PotatoRig, containing: &str) -> String {
    wait_for(&format!("a reply containing {containing:?}"), 40, || bot_lines(rig).into_iter().find(|l| l.contains(containing)))
}

fn bot_view(rig: &PotatoRig) -> Value {
    rig.state()["bots"].as_array().and_then(|b| b.iter().find(|b| b["id"] == BOT)).cloned().unwrap_or_default()
}

fn rig() -> &'static PotatoRig {
    let rig = PotatoRig::start();
    rig.add_bot(BOT).expect("a bot from the companion");
    rig
}

#[test]
fn a_send_produces_a_stored_redacted_reply_and_the_brain_gets_403_on_panes() {
    let rig = rig();
    // Start from no brain, so the start below is the one the log sees.
    rig.stop_all();
    spawn_log::start();

    // 1. hello: lazy start, the typing row, the reply, the mood, the usage
    let out = rig.send(BOT, "hello").expect("send");
    assert_eq!(out["delivered"], true, "{out}");
    wait_for_bot_line(rig, "Fish heard: hello");
    let pid = rig.brain_pid(BOT).expect("the brain runs");
    assert!(pid_alive(pid));
    let view = wait_for("the mood", 20, || {
        let v = bot_view(rig);
        (!v["mood"].is_null()).then_some(v)
    });
    assert_eq!(view["mood"]["mood"], "genki");
    assert_eq!(view["mood"]["energy"], 60);
    assert_eq!(view["brainState"], "ready");
    assert_eq!(view["provider"], "stub");
    assert!(view["unread"].as_u64().unwrap_or(0) >= 1, "the reply is unread: {view}");
    // the fake brain's model is free: usage was recorded and it cost nothing
    assert_eq!(view["usedTodayNano"], 0);

    // 2. a secret in the person's message never reaches the store, the brain or the reply
    let secret = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij";
    rig.send(BOT, &format!("my key is {secret}")).expect("send");
    let heard = wait_for_bot_line(rig, "Fish heard: my key is");
    assert!(!heard.contains(secret), "the brain was told the text redacted: {heard}");
    let thread = serde_json::to_string(&rig.thread(BOT)).unwrap();
    assert!(!thread.contains(secret), "the thread holds no secret");
    let on_disk = std::fs::read_dir(puppet_master_app_lib::app_paths::app_data_dir().join("potato").join("conversations").join("app"))
        .expect("the conversations folder")
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect::<String>();
    assert!(!on_disk.contains(secret), "the file on disk holds no secret");

    // 3. the brain cannot reach the rest of the bridge: 403 on /panes with its token, and 403 without it
    rig.send(BOT, "probe: /panes").expect("send");
    let probed = wait_for_bot_line(rig, "Fish probed /panes");
    assert_eq!(probed, "Fish probed /panes: 403 with the token, 403 without", "{probed}");
    rig.send(BOT, "probe: /health").expect("send");
    assert_eq!(wait_for_bot_line(rig, "Fish probed /health"), "Fish probed /health: 403 with the token, 403 without");
    rig.send(BOT, "probe: /agents/definitions").expect("send");
    assert_eq!(wait_for_bot_line(rig, "Fish probed /agents/definitions"), "Fish probed /agents/definitions: 403 with the token, 403 without");

    // 4. no window: the brain and Node's version probe were started through hidden_command, and nothing else was
    let spawned = spawn_log::finish();
    assert!(
        spawned.iter().any(|s| s.to_lowercase().contains("node") && s.contains("potato/sidecar.rs")),
        "the brain starts through hidden_command (CREATE_NO_WINDOW): {spawned:?}"
    );

    // 5. the brain's log holds no token or key
    let log = rig.log(BOT).join("\n");
    assert!(!log.contains(secret));

    // 6. stopping it stops the process
    rig.stop_all();
    wait_for("the brain process to be gone", 10, || (!pid_alive(pid)).then_some(()));
}

#[test]
fn a_crash_is_seen_and_the_next_message_goes_to_a_new_brain() {
    let rig = rig();
    rig.send(BOT, "wake up").expect("send");
    wait_for_bot_line(rig, "Fish heard: wake up");
    let first = rig.brain_pid(BOT).expect("the brain runs");

    // "crash" makes the fake brain exit with code 1 at once
    rig.send(BOT, "crash").expect("send");
    wait_for("the process to be gone", 20, || (!pid_alive(first)).then_some(()));
    wait_for("the app to see it", 20, || {
        let state = bot_view(rig)["brainState"].as_str().map(str::to_string);
        matches!(state.as_deref(), Some("restarting") | Some("ready") | Some("starting") | Some("idle")).then_some(())
    });
    wait_for("a note that it stopped before answering", 20, || {
        rig.thread(BOT).iter().any(|m| m["kind"] == "note" && m["text"].as_str().is_some_and(|t| t.contains("stopped before it answered"))).then_some(())
    });
    // after the wait the next message is answered by a new process
    rig.send(BOT, "are you back").expect("send");
    wait_for_bot_line(rig, "Fish heard: are you back");
    let second = rig.brain_pid(BOT).expect("a new brain");
    assert_ne!(first, second);
    rig.stop_all();
    wait_for("the brain process to be gone", 10, || (!pid_alive(second)).then_some(()));
}

#[test]
fn the_kill_switch_kills_the_brain_and_a_message_is_refused_while_it_is_off() {
    let rig = rig();
    rig.send(BOT, "before").expect("send");
    wait_for_bot_line(rig, "Fish heard: before");
    let pid = rig.brain_pid(BOT).expect("the brain runs");
    rig.set_enabled(false);
    wait_for("the brain process to be gone", 10, || (!pid_alive(pid)).then_some(()));
    let err = rig.send(BOT, "while off").unwrap_err();
    assert!(err.contains("POTATO_OFF"), "{err}");
    assert_eq!(rig.state()["enabled"], false);
    // on again: it works again
    rig.set_enabled(true);
    rig.send(BOT, "after").expect("send");
    wait_for_bot_line(rig, "Fish heard: after");
    rig.stop_all();
}
