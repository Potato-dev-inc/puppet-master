//! The opt-in live test: the REAL code path (the real `Wsl` with the real runner, the real guest files, a real
//! distro named `pm-sbx-e2e`). It is `#[ignore]` and also needs `PM_SBX_LIVE=1`:
//!
//! ```text
//! PM_SBX_LIVE=1 cargo test --lib potato::sandbox::live -- --ignored --nocapture
//! ```
//!
//! What it proves, in order: install (base + the light desktop) -> start -> readiness (the guest's boot verdict) ->
//! a screenshot through the control client -> a click and typing through the control client (the typed command
//! creates a file, so the keys really reached a program) -> the screen target answers on its loopback port and the
//! app's own passwords are the ones x11vnc holds -> the desktop script stops Chrome -> stop -> remove (unregister
//! `pm-sbx-e2e` and nothing else). Between the screen target and the Chrome stop it also runs the bot's side THROUGH THE REAL
//! PATH: the host's wiring (`sandbox_host::wire_into`: one sandbox, the screen hub's one lease), S3's dispatcher
//! (`sandbox_calls::Calls`) with `browser_navigate https://example.com` and `browser_snapshot` (the page must say
//! "Example Domain"), the "Needs you" card, and S2's screen route in process serving `embed.html` for a sealed link made by
//! the hub. No window may appear on the person's screens: the visible remote-desktop windows
//! (WSLg's `msrdc`) are counted before, during the start and after, and every count must be 0.
//!
//! It touches only the distro `pm-sbx-e2e`. Pictures go to `D:\pm-sandbox\e2e\`. Not part of the normal test run.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::control::Action;
use super::install::{Desktop, GuestFiles, InstallOptions, Layout};
use super::lifecycle::{Lifecycle, START_DESKTOP};
use super::wsl::{GuestUser, SandboxName, Wsl};
use super::screen::{Audience, Mode, ScreenHub};
use super::{Sandbox, WslSandbox};
use crate::potato::protocol::Trust;
use crate::potato::sandbox_calls::{Calls, Context, Gates};
use crate::potato::service::Host;
use crate::potato::service_tests::FakeHost;
use crate::potato::window_guard;

const DISTRO: &str = "pm-sbx-e2e";
/// The folder the distro lives under (`<this>/sandbox/pm-sbx-e2e`).
const APP_DATA: &str = r"D:\pm-sandbox\pm-sbx-e2e";
const OUT_DIR: &str = r"D:\pm-sandbox\e2e";

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(OUT_DIR);
    std::fs::create_dir_all(&dir).expect("make the output folder");
    dir
}

/// How many visible msrdc windows there are right now, and the report line (the production check, `window_guard`).
fn visible_msrdc_windows() -> Result<(usize, String), String> {
    window_guard::visible_windows(&window_guard::PowerShell)
}

/// Removes the e2e distro (that name and no other) if the test ends early.
struct Cleanup {
    wsl: Wsl,
    name: SandboxName,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.wsl.is_installed(&self.name).unwrap_or(false) {
            println!("[live] cleanup: unregistering {}", self.name);
            let _ = self.wsl.terminate(&self.name);
            let _ = self.wsl.unregister(&self.name);
        }
    }
}

fn root_text(wsl: &Wsl, name: &SandboxName, argv: &[&str]) -> String {
    let out = wsl.exec(name, GuestUser::Root, argv, None, Duration::from_secs(30)).expect("root call");
    out.stdout_text().trim().to_string()
}

fn pm_ok(wsl: &Wsl, name: &SandboxName, argv: &[&str]) -> (bool, String) {
    let out = wsl.exec(name, GuestUser::Pm, argv, None, Duration::from_secs(60)).expect("pm call");
    (out.success(), out.stdout_text().trim().to_string())
}

fn chrome_count(wsl: &Wsl, name: &SandboxName) -> usize {
    let text = root_text(wsl, name, &["/usr/bin/pgrep", "-u", "pm", "-f", "^/opt/google/chrome/chrome( |$)"]);
    text.lines().filter(|l| !l.trim().is_empty()).count()
}

fn http_get(port: u16) -> String {
    let addr = ("127.0.0.1", port).to_socket_addrs().expect("loopback address").next().expect("an address");
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("connect to the screen port");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(b"GET / HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
    let mut body = Vec::new();
    let _ = stream.take(256 * 1024).read_to_end(&mut body);
    String::from_utf8_lossy(&body).to_string()
}

fn save(name: &str, png: &[u8]) -> PathBuf {
    let path = out_dir().join(name);
    std::fs::write(&path, png).expect("save the picture");
    path
}

fn guest_files() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox")
}

fn show_list(label: &str, wsl: &Wsl) {
    let rows: Vec<String> = wsl.list().expect("wsl -l -v").iter().map(|d| format!("{} {}", d.name, d.state)).collect();
    println!("[live] wsl -l -v {label}: {}", rows.join(" | "));
}

/// The bot's side of the real path against the running sandbox: the host's wiring (one sandbox, the hub's one lease), the
/// dispatcher with the page tools, the "Needs you" card, and the screen route serving the viewer page for a sealed link.
fn bots_side_through_the_real_path(sandbox: &Arc<WslSandbox>) {
    use std::collections::HashMap;

    use parking_lot::Mutex;
    use serde_json::json;

    let host = Arc::new(FakeHost {
        now: AtomicI64::new(1_000),
        secrets: Mutex::new(HashMap::new()),
        changes: Mutex::new(Vec::new()),
        resources: Mutex::new(None),
        url: Mutex::new(None),
    });
    let clock = host.clone();
    let hub = Arc::new(ScreenHub::with_clock(Box::new(move || clock.now.load(Ordering::SeqCst))));
    hub.set_enabled(true);
    let as_dyn: Arc<dyn Sandbox> = sandbox.clone();
    let port = crate::potato::sandbox_host::wire_into(&hub, Some(&as_dyn)).expect("the host's port");
    assert!(Arc::ptr_eq(&port.lease, hub.lease()), "one lease");

    let root = out_dir().join("calls");
    std::fs::create_dir_all(&root).unwrap();
    let calls = Calls::new(root, host.clone() as Arc<dyn Host>);
    let ctx = Context { bot: "fish".to_string(), conversation: "app:dm:fish".to_string(), trust: Trust::OwnerPc, want_image: false };
    let gates = Gates { bot_on: true, sandbox_on: true };

    // browser_navigate + browser_snapshot through S3's dispatcher with the real sandbox
    let t = std::time::Instant::now();
    let navigated = calls.execute(Some(&port), gates, &ctx, "browser_navigate", &json!({ "url": "https://example.com" })).expect("browser_navigate https://example.com");
    println!("[live] browser_navigate took {:?}: {}", t.elapsed(), navigated.to_string().chars().take(300).collect::<String>());
    assert_ne!(navigated["ok"], json!(false), "navigate did not fail: {navigated}");
    let t = std::time::Instant::now();
    let snapshot = calls.execute(Some(&port), gates, &ctx, "browser_snapshot", &json!({})).expect("browser_snapshot");
    let text = snapshot.to_string();
    println!("[live] browser_snapshot took {:?}: {}", t.elapsed(), text.chars().take(400).collect::<String>());
    assert!(text.contains("Example Domain"), "the page is example.com: {text}");

    // the dispatcher took the hub's lease
    let state = hub.state();
    println!("[live] screen state after the calls: {state:?}");
    assert_eq!((state.phase, state.bot.as_deref()), ("bot", Some("fish")), "the bot holds the hub's one lease");

    // the sixth tool raises the card on that same lease
    let asked = calls.execute(Some(&port), gates, &ctx, "ask_person_for_screen", &json!({ "reason": "live test: please look at the page" })).expect("ask_person_for_screen");
    assert_eq!(asked["asked"], json!(true), "{asked}");
    let state = hub.state();
    assert_eq!(state.phase, "needs_you");
    hub.skip(state.card_id.as_deref().expect("a card id")).expect("skip");
    assert_eq!(hub.state().phase, "bot", "Skip keeps the bot's hold");

    // S2's screen route in process: the sealed link of the hub serves the viewer page
    let src = hub.link("fish", Mode::View, &Audience::App, "").expect("a screen link").into_inner();
    let start = src.find("/screen/").expect("a screen path") + "/screen/".len();
    let sealed = src[start..].split('/').next().unwrap().to_string();
    let (status, head, body) = crate::bridge::screen_get_in_process(&hub, &sealed, "embed.html");
    println!("[live] GET screen/<sealed>/embed.html -> {status}, {} bytes, no-store: {}", body.len(), head.to_lowercase().contains("no-store"));
    assert_eq!(status, 200, "{head}");
    assert!(body.to_lowercase().contains("<html") && body.to_lowercase().contains("novnc"), "the viewer page");
    let passwords = sandbox.screen_target().expect("target").passwords.expect("passwords");
    assert!(!body.contains(passwords.view().expose()) && !body.contains(passwords.control().expose()) && !head.contains(passwords.view().expose()), "no password in the answer");
    println!("[live] the bot's side works through the real path");
}

#[test]
#[ignore = "needs WSL, downloads Ubuntu and Chrome, takes minutes: PM_SBX_LIVE=1 cargo test --lib potato::sandbox::live -- --ignored --nocapture"]
fn a_real_distro_installs_starts_is_driven_and_is_removed() {
    if std::env::var("PM_SBX_LIVE").ok().as_deref() != Some("1") {
        println!("[live] skipped: set PM_SBX_LIVE=1 to run against a real WSL distro");
        return;
    }
    let started = std::time::Instant::now();
    let wsl = Wsl::live().expect("this computer has wsl.exe");
    let name = SandboxName::new(DISTRO).unwrap();
    show_list("before", &wsl);

    let (windows_before, line) = visible_msrdc_windows().expect("count windows before");
    println!("[live] before: {line}");
    assert_eq!(windows_before, 0, "no visible remote-desktop window before the test");

    // The e2e name is ours alone: a leftover of an earlier failed run is removed first.
    if wsl.is_installed(&name).unwrap() {
        println!("[live] a leftover {DISTRO} is removed first");
        let _ = wsl.terminate(&name);
        wsl.unregister(&name).unwrap();
    }
    let _cleanup = Cleanup { wsl: wsl.clone(), name: name.clone() };

    let layout = Layout::new(Path::new(APP_DATA));
    let files = GuestFiles::new(&guest_files());
    files.check().expect("the guest files are in the repo");
    let lifecycle = Arc::new(Lifecycle::new(wsl.clone(), name.clone()).with_window_check(Arc::new(window_guard::Msrdc::live())));
    let sandbox = Arc::new(WslSandbox::new(wsl.clone(), name.clone(), layout, files, lifecycle.clone()));

    // ---- install (base + the light desktop), with the real start check inside it
    let mut options = InstallOptions::new(name.clone(), Desktop::Light);
    options.snapshot = false;
    let mut last = String::new();
    let t = std::time::Instant::now();
    sandbox
        .install(&options, &mut |p| {
            let line = format!("{}% {} {}", p.percent, p.title, p.detail.unwrap_or_default());
            if line != last {
                println!("[live] install {line}");
                last = line;
            }
        })
        .expect("install");
    println!("[live] install took {:?}", t.elapsed());
    show_list("after install", &wsl);

    // ---- start -> readiness
    let t = std::time::Instant::now();
    let ready = lifecycle.ensure_ready(0).expect("start and readiness");
    println!("[live] start + readiness took {:?}", t.elapsed());
    let (windows_during, line) = visible_msrdc_windows().expect("count windows during");
    println!("[live] while running: {line}");
    assert_eq!(windows_during, 0, "no visible remote-desktop window while the sandbox runs");
    show_list("running", &wsl);

    // the files the app pushed: owned by pm, mode 0400; the desktop script used them, not its fallback
    let stat = root_text(&wsl, &name, &["/usr/bin/stat", "-c", "%U %a %n", "/run/pm-sbx/control.token", "/run/pm-sbx/vnc-view.pass", "/run/pm-sbx/vnc-control.pass"]);
    println!("[live] pushed files:\n{stat}");
    for line in stat.lines() {
        assert!(line.starts_with("pm 400 "), "{line}");
    }
    let start_log = root_text(&wsl, &name, &["/bin/cat", "/home/pm/.pm-sbx/logs/start.log"]);
    println!("[live] start.log:\n{start_log}");
    assert!(!start_log.contains("FALLBACK"), "the guest invented a secret the app gave");
    assert!(start_log.contains("passwords: from the app") && start_log.contains("token: from the app"), "{start_log}");

    // ---- screenshot through the control client
    let first = ready.client.screenshot().expect("a screenshot");
    assert_eq!(&first.png[..4], b"\x89PNG");
    assert_eq!((first.width, first.height), (1440, 900));
    println!("[live] screenshot 1: {}x{} {} bytes -> {}", first.width, first.height, first.png.len(), save("e2e-1-desktop.png", &first.png).display());

    // ---- click + type through the control client: open a terminal, click in it, type a command that makes a file
    let typed_file = "/tmp/pm-e2e-typed";
    let actions = vec![
        Action::Launch { app: "terminal".to_string(), arg: None },
        Action::Wait { ms: 1500 },
        Action::Click { x: 300, y: 300, button: super::control::Button::Left },
        Action::Type { text: format!("touch {typed_file} && echo hello-from-pm-sbx-e2e") },
        Action::Key { combo: "Return".to_string() },
    ];
    let result = ready.client.act(&actions, 800, true).expect("click and type");
    let second = result.observation.expect("a picture after the actions");
    println!(
        "[live] act: completed {} steps; pointer {:?}; front window {:?}; {} bytes -> {}",
        result.completed,
        second.pointer,
        second.active_window_title,
        second.png.len(),
        save("e2e-2-after-click-and-type.png", &second.png).display()
    );
    assert_eq!(second.pointer, Some((300, 300)), "the click moved the pointer");
    assert_ne!(first.png, second.png, "the screen changed");
    let exists = wsl.exec(&name, GuestUser::Root, &["/usr/bin/test", "-f", typed_file], None, Duration::from_secs(15)).unwrap();
    assert!(exists.success(), "the typed command ran: {typed_file} exists");
    println!("[live] the typed command created {typed_file}: the click and the keys reached a program");

    // ---- the screen target answers and holds the app's passwords
    let target = sandbox.screen_target().expect("screen target");
    println!("[live] screen target: view port {} generation {} passwords {:?}", target.view_port, target.generation, target.passwords);
    let page = http_get(target.view_port);
    let first_line = page.lines().next().unwrap_or("").to_string();
    println!("[live] GET http://127.0.0.1:{}/ -> {first_line}", target.view_port);
    assert!(first_line.contains("200"), "the viewer answers: {first_line}");
    assert!(page.to_lowercase().contains("novnc") || page.to_lowercase().contains("<html"), "an html page");
    let passwords = target.passwords.as_ref().expect("the passwords while it runs");
    let view_file = root_text(&wsl, &name, &["/bin/cat", "/home/pm/.pm-sbx/vnc/view.plain"]);
    let control_file = root_text(&wsl, &name, &["/bin/cat", "/home/pm/.pm-sbx/vnc/control.plain"]);
    assert_eq!(view_file, passwords.view().expose(), "x11vnc holds the app's view password");
    assert_eq!(control_file, passwords.control().expose(), "x11vnc holds the app's control password");
    assert_ne!(view_file, control_file);
    println!("[live] x11vnc holds the app's two passwords (not printed)");

    bots_side_through_the_real_path(&sandbox);

    // ---- stopping the desktop stops Chrome too
    let _ = ready.client.act(&[Action::Launch { app: "chrome".to_string(), arg: Some("about:blank".to_string()) }, Action::Wait { ms: 3000 }], 500, false);
    let mut chrome = 0;
    for _ in 0..20 {
        chrome = chrome_count(&wsl, &name);
        if chrome > 0 {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("[live] Chrome processes before the desktop stop: {chrome}");
    assert!(chrome > 0, "Chrome started");
    let (stopped, text) = pm_ok(&wsl, &name, &[START_DESKTOP, "stop"]);
    println!("[live] pm-sbx-start-desktop stop -> {stopped} {text}");
    assert!(stopped);
    let left = chrome_count(&wsl, &name);
    println!("[live] Chrome processes after the desktop stop: {left}");
    assert_eq!(left, 0, "stopping the desktop stopped Chrome");
    let (_, status) = pm_ok(&wsl, &name, &[START_DESKTOP, "status"]);
    assert!(status.contains("xvfb stopped") && status.contains("control stopped"), "{status}");

    // ---- stop -> remove
    let (windows_after_run, line) = visible_msrdc_windows().expect("count windows after the run");
    println!("[live] after the run: {line}");
    assert_eq!(windows_after_run, 0);
    sandbox.stop().expect("stop");
    show_list("after stop", &wsl);
    assert!(!wsl.is_running(&name).unwrap(), "stopped");
    sandbox.remove().expect("remove");
    assert!(!wsl.is_installed(&name).unwrap(), "{DISTRO} is gone");
    show_list("after remove", &wsl);
    let (windows_after, line) = visible_msrdc_windows().expect("count windows after remove");
    println!("[live] after remove: {line}");
    assert_eq!(windows_after, 0);
    println!("[live] DONE in {:?}", started.elapsed());
}
