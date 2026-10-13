use super::*;
use std::sync::Arc;

/// The tracker state is process-global, so tests that observe run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

// Real-looking dev-server output (formats as printed by each tool, ANSI already stripped by vt100).
const VITE: &str = "\n  VITE v5.4.8  ready in 312 ms\n\n  ➜  Local:   http://localhost:5173/\n  ➜  Network: http://192.168.1.23:5173/\n  ➜  press h + enter to show help\n";
const NEXT: &str = "  ▲ Next.js 14.2.5\n  - Local:        http://localhost:3000\n  - Network:      http://192.168.0.14:3000\n  - Environments: .env.local\n\n ✓ Starting...\n ✓ Ready in 2.1s\n";
const CRA: &str = "Compiled successfully!\n\nYou can now view my-app in the browser.\n\n  Local:            http://localhost:3000\n  On Your Network:  http://10.0.0.5:3000\n\nNote that the development build is not optimized.\n";
const DJANGO: &str = "Watching for file changes with StatReloader\nPerforming system checks...\n\nSystem check identified no issues (0 silenced).\nOctober 14, 2026 - 03:12:44\nDjango version 5.1, using settings 'mysite.settings'\nStarting development server at http://127.0.0.1:8000/\nQuit the server with CTRL-BREAK.\n";
const FLASK: &str = " * Serving Flask app 'app'\n * Debug mode: on\nWARNING: This is a development server. Do not use it in a production deployment.\n * Running on all addresses (0.0.0.0)\n * Running on http://127.0.0.1:5000\n * Running on http://172.20.10.4:5000\nPress CTRL+C to quit\n";
const UVICORN: &str = "INFO:     Uvicorn running on http://0.0.0.0:8000 (Press CTRL+C to quit)\nINFO:     Started reloader process\n";
const ASTRO: &str = " astro  v4.15.0 started in 120ms\n\n  ┃ Local    http://localhost:4321/\n  ┃ Network  use --host to expose\n";
const VITE_IPV6: &str = "  ➜  Local:   http://[::1]:5173/\n";

fn urls(tail: &str) -> Vec<(String, Option<String>)> {
    detect_urls(tail).into_iter().map(|p| (p.url, p.network_url)).collect()
}

#[test]
fn vite_prints_local_and_network_as_one_preview() {
    assert_eq!(urls(VITE), vec![("http://localhost:5173/".to_string(), Some("http://192.168.1.23:5173/".to_string()))]);
}

#[test]
fn next_and_cra_keep_the_local_url_and_the_lan_one() {
    assert_eq!(urls(NEXT), vec![("http://localhost:3000".to_string(), Some("http://192.168.0.14:3000".to_string()))]);
    assert_eq!(urls(CRA), vec![("http://localhost:3000".to_string(), Some("http://10.0.0.5:3000".to_string()))]);
}

#[test]
fn django_flask_uvicorn_and_astro_are_found_and_loopback_is_normalised() {
    assert_eq!(urls(DJANGO), vec![("http://localhost:8000/".to_string(), None)]);
    assert_eq!(
        urls(FLASK),
        vec![("http://localhost:5000".to_string(), Some("http://172.20.10.4:5000".to_string()))]
    );
    // 0.0.0.0 is not something a browser can open: it is shown as localhost
    assert_eq!(urls(UVICORN), vec![("http://localhost:8000".to_string(), None)]);
    assert_eq!(urls(ASTRO), vec![("http://localhost:4321/".to_string(), None)]);
    assert_eq!(urls(VITE_IPV6), vec![("http://localhost:5173/".to_string(), None)]);
}

#[test]
fn public_urls_and_non_http_text_are_ignored() {
    let tail = "see https://vitejs.dev/guide and docs at http://example.com:8080/x\nlistening on tcp://127.0.0.1:9000\nport 3000 is in use\nhttp://8.8.8.8:80/";
    assert!(detect_urls(tail).is_empty());
}

#[test]
fn trailing_punctuation_and_paths_are_handled() {
    assert_eq!(urls("Open http://localhost:8080/app/index.html, then reload.")[0].0, "http://localhost:8080/app/index.html");
    assert_eq!(urls("(http://localhost:3000)")[0].0, "http://localhost:3000");
    assert_eq!(urls("https://localhost:8443/admin.")[0].0, "https://localhost:8443/admin");
}

#[test]
fn ports_are_validated() {
    assert!(detect_urls("http://localhost:99999/").is_empty());
    assert!(detect_urls("http://localhost:0/").is_empty());
}

fn capture() -> (Arc<Mutex<Vec<(String, usize)>>>, ()) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink_log = log.clone();
    install_sink(Box::new(move |id, urls| sink_log.lock().push((id.to_string(), urls.len()))));
    (log, ())
}

#[test]
fn a_pane_keeps_its_three_most_recent_distinct_servers() {
    let _serial = SERIAL.lock();
    let (log, _) = capture();
    for port in [3000, 3001, 3002, 3003] {
        observe("cap-1", &format!("Local: http://localhost:{port}/"));
    }
    let urls: Vec<u16> = snapshot()["cap-1"].iter().map(|p| p.port).collect();
    assert_eq!(urls, vec![3003, 3002, 3001], "newest first, oldest dropped");
    assert_eq!(log.lock().len(), 4, "one event per change");
    retain_alive(&|_| false);
}

#[test]
fn seeing_the_same_server_again_is_silent_and_does_not_reshuffle() {
    let _serial = SERIAL.lock();
    let (log, _) = capture();
    observe("dd-1", "http://localhost:5173/");
    observe("dd-1", "http://localhost:3000/\nhttp://localhost:5173/");
    let before = log.lock().len();
    for _ in 0..50 {
        observe("dd-1", "http://localhost:5173/\nhttp://localhost:3000/");
    }
    assert_eq!(log.lock().len(), before, "no event for repeats");
    let ports: Vec<u16> = snapshot()["dd-1"].iter().map(|p| p.port).collect();
    assert_eq!(ports, vec![3000, 5173]);
    // a LAN address that shows up later is merged into the same entry
    observe("dd-1", "Network: http://192.168.1.5:3000/");
    assert_eq!(snapshot()["dd-1"][0].network_url.as_deref(), Some("http://192.168.1.5:3000/"));
    retain_alive(&|_| false);
}

#[test]
fn output_without_a_url_changes_nothing() {
    let _serial = SERIAL.lock();
    let (log, _) = capture();
    observe("quiet-1", "compiling...\nok\n");
    assert!(!snapshot().contains_key("quiet-1"));
    assert!(log.lock().is_empty());
}

#[test]
fn urls_expire_when_the_pane_exits_and_listeners_hear_it() {
    let _serial = SERIAL.lock();
    let (log, _) = capture();
    observe("exp-1", VITE);
    observe("exp-2", NEXT);
    assert!(snapshot().contains_key("exp-1") && snapshot().contains_key("exp-2"));
    // exp-1 exits, exp-2 keeps running
    retain_alive(&|id| id != "exp-1");
    assert!(!snapshot().contains_key("exp-1"));
    assert!(snapshot().contains_key("exp-2"));
    assert!(log.lock().contains(&("exp-1".to_string(), 0)), "an empty list is sent so the UI clears it");
    retain_alive(&|_| false);
    assert!(!snapshot().contains_key("exp-2"));
}

#[test]
fn the_activity_ticker_feeds_the_detector_once_per_changed_screen() {
    // Throttle reuse: observe() is only reached through tick_with, i.e. for dirty panes whose
    // screen hash changed, never per byte.
    let _serial = SERIAL.lock();
    let _a = crate::pane_activity::TEST_SERIAL.lock();
    let (_log, _) = capture();
    let screen = std::cell::RefCell::new(VITE.to_string());
    let read = |_: &str| {
        Some(crate::pane_activity::PaneSnapshot {
            agent_type: "cursor_agent".into(),
            screen: screen.borrow().clone(),
            status: crate::pty::status::PaneStatus::Running,
        })
    };
    let noop = |_: &str, _: &crate::tui::Activity| {};
    for _ in 0..5000 {
        crate::pane_activity::note_output("tick-1");
    }
    assert_eq!(crate::pane_activity::tick_with(&read, &|_| true, &noop), 1);
    assert_eq!(snapshot()["tick-1"][0].port, 5173);
    retain_alive(&|_| false);
}
