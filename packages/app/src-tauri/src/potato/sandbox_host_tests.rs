//! One sandbox, one lease. The host (`sandbox_host.rs`) gives the screen hub the app's sandbox and gives the brain's
//! tools a `Port` made of that same sandbox and the hub's own lease table. These tests prove the three parties see
//! one lease: the hub (the Computer panel, the screen links), the dispatcher (`sandbox_calls`, the bot's tools) and the
//! host. A fake sandbox stands in for Linux and the hub's clock is the engine's fake clock.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::sandbox::manager::{Events, InstallView, Manager, NoEvents, WizardAnswers};
use super::sandbox::screen::{self, Audience, Mode, ScreenHub};
use super::sandbox::wsl::SandboxName;
use super::sandbox::{FakeSandbox, Recorded, Sandbox};
use super::sandbox_host::{sync_into, usable_of, wire_into};
use super::service_tests::Rig;

fn fake(installed: bool) -> Arc<dyn Sandbox> {
    if installed {
        Arc::new(FakeSandbox::new())
    } else {
        Arc::new(FakeSandbox::not_installed())
    }
}

fn manager_of(sandbox: Arc<dyn Sandbox>, events: Arc<dyn Events>) -> Arc<Manager> {
    Manager::new(sandbox, SandboxName::main(), None, None, events)
}

/// The error the hub gives for a screen link: tells whether the hub has a sandbox, and whether it runs.
fn link_error(hub: &ScreenHub) -> &'static str {
    hub.link("nimbus", Mode::View, &Audience::App, "").map(|_| "ok").unwrap_or_else(|e| e.code())
}

#[test]
fn the_port_is_the_same_sandbox_and_the_hubs_own_lease() {
    let hub = ScreenHub::new();
    hub.set_enabled(true);
    assert_eq!(link_error(&hub), "SANDBOX_NOT_INSTALLED", "no sandbox yet");
    let sandbox = fake(true);
    let port = wire_into(&hub, Some(&sandbox)).expect("a port");
    assert!(Arc::ptr_eq(&port.lease, hub.lease()), "ONE lease: the port's table is the hub's table");
    assert!(Arc::ptr_eq(&port.sandbox, &sandbox), "ONE sandbox");
    assert_eq!(link_error(&hub), "SCREEN_NOT_RUNNING", "the hub has the sandbox now (the fake has no screen passwords)");

    // unwired: no port, and the hub has nothing again
    assert!(wire_into(&hub, None).is_none());
    assert_eq!(link_error(&hub), "SANDBOX_NOT_INSTALLED");
}

#[test]
fn the_hub_the_dispatcher_and_the_host_see_one_lease() {
    let rig = Rig::new("host-one-lease");
    rig.svc.set_spawner(Arc::new(|job| job()));
    let host = rig.host.clone();
    let hub = ScreenHub::with_clock(Box::new(move || host.now.load(Ordering::SeqCst)));
    hub.set_enabled(true);
    let sandbox = fake(true);
    let manager = manager_of(sandbox.clone(), Arc::new(NoEvents));
    // the host's wiring, as `sync` does it
    let port = sync_into(true, Some(&manager), &hub);
    assert!(port.is_some());
    rig.svc.set_sandbox_port(port.clone());
    rig.svc.set_sandbox_enabled(true);
    rig.svc.send("nimbus", "go").unwrap();
    rig.hello();

    let call = |id: &str, tool: &str, args: Value| json!({"v":1,"type":"sandbox_call","callId":id,"conversationId":"app:dm:nimbus","tool":tool,"args":args,"image":false});
    let result = |id: &str| rig.inbox(0).0.into_iter().find(|i| i["type"] == "sandbox_result" && i["callId"] == id).unwrap_or_else(|| panic!("no result {id}"));

    // 1. the dispatcher takes the lease: the hub (the panel's header) says the bot is using the screen
    rig.post(vec![call("c-1", "computer_observe", json!({}))]);
    assert_eq!(result("c-1")["ok"], true);
    let state = hub.state();
    assert_eq!((state.phase, state.bot.as_deref()), ("bot", Some("nimbus")));

    // 2. the bot asks for help: the panel's state is "Needs you" with the reason
    rig.post(vec![call("c-2", "ask_person_for_screen", json!({ "reason": "A code is needed" }))]);
    assert_eq!(result("c-2")["result"]["asked"], true);
    let state = hub.state();
    assert_eq!((state.phase, state.reason.as_deref()), ("needs_you", Some("A code is needed")));

    // 3. the person takes over in the hub: the dispatcher sees it and the bot is told to wait
    hub.take_over().unwrap();
    assert_eq!(hub.state().phase, "you");
    rig.post(vec![call("c-3", "computer_observe", json!({}))]);
    assert_eq!(result("c-3")["error"]["code"], "LEASE_HELD_BY_PERSON");
    assert_eq!(result("c-3")["error"]["needsYou"], true);

    // 4. the person hands it back in the hub: the dispatcher gets a new lease (after one "look again")
    hub.hand_back().unwrap();
    assert_eq!(hub.state().phase, "free");
    rig.post(vec![call("c-4", "computer_observe", json!({}))]);
    assert_eq!(result("c-4")["error"]["code"], "LEASE_LOST");
    rig.post(vec![call("c-5", "computer_observe", json!({}))]);
    assert_eq!(result("c-5")["ok"], true);
    assert_eq!(hub.state().phase, "bot");

    // 5. the host sees the same table: the port it made is the hub's
    assert!(Arc::ptr_eq(&port.unwrap().lease, hub.lease()));
}

#[test]
fn the_app_wide_hub_and_the_live_port_are_one_lease() {
    // the real statics: what `sandbox_host::sync` does, through `live`
    let sandbox = fake(true);
    let port = wire_into(screen::hub(), Some(&sandbox));
    super::live::install_sandbox(port);
    let wired = super::live::sandbox_port().expect("the port is installed");
    assert!(Arc::ptr_eq(&wired.lease, screen::hub().lease()));
    assert!(Arc::ptr_eq(&wired.sandbox, &sandbox));
    super::live::install_sandbox(wire_into(screen::hub(), None));
    assert!(super::live::sandbox_port().is_none(), "unwired again");
}

/// Wires a manager's hub on every change, the way the host's events do (with a local hub, not the app's).
struct WireOnChange {
    hub: Arc<ScreenHub>,
    manager: Mutex<Option<Arc<Manager>>>,
    ports: Mutex<Vec<bool>>,
}

impl Events for WireOnChange {
    fn install_progress(&self, _view: &InstallView) {}

    fn changed(&self) {
        let manager = self.manager.lock().clone();
        let port = sync_into(true, manager.as_ref(), &self.hub);
        self.ports.lock().push(port.is_some());
    }
}

#[test]
fn nothing_is_offered_until_the_sandbox_is_set_up_and_not_after_it_is_removed() {
    let hub = Arc::new(ScreenHub::new());
    hub.set_enabled(true);
    let events = Arc::new(WireOnChange { hub: hub.clone(), manager: Mutex::new(None), ports: Mutex::new(Vec::new()) });
    let sandbox = fake(false);
    let manager = manager_of(sandbox.clone(), events.clone());
    *events.manager.lock() = Some(manager.clone());

    // made, not set up: no port, nothing for the panel
    assert!(sync_into(true, Some(&manager), &hub).is_none());
    assert_eq!(link_error(&hub), "SANDBOX_NOT_INSTALLED");

    // the wizard's install ends: the manager says "changed" and the wiring follows
    let options = manager.prepare_install(&WizardAnswers { look: "ubuntu".to_string(), uses: vec!["coding".to_string()], ..WizardAnswers::default() }).unwrap();
    manager.run_install(&options);
    assert_eq!(*events.ports.lock(), vec![true], "installed: the tools get a port");
    assert_eq!(link_error(&hub), "SCREEN_NOT_RUNNING", "and the panel has the sandbox");

    // stop and start keep it wired
    manager.start().unwrap();
    manager.stop().unwrap();
    assert_eq!(*events.ports.lock(), vec![true, true, true]);

    // removed: both let go
    manager.remove("remove").unwrap();
    assert_eq!(*events.ports.lock(), vec![true, true, true, false], "removed: no port");
    assert_eq!(link_error(&hub), "SANDBOX_NOT_INSTALLED");
}

#[test]
fn labs_off_offers_nothing_even_when_a_sandbox_is_set_up() {
    let hub = ScreenHub::new();
    hub.set_enabled(true);
    let manager = manager_of(fake(true), Arc::new(NoEvents));
    assert!(usable_of(false, Some(&manager)).is_none());
    assert!(sync_into(true, Some(&manager), &hub).is_some());
    assert_eq!(link_error(&hub), "SCREEN_NOT_RUNNING");
    assert!(sync_into(false, Some(&manager), &hub).is_none(), "switch off: no port");
    assert_eq!(link_error(&hub), "SANDBOX_NOT_INSTALLED", "switch off: the panel has no sandbox");
    assert!(usable_of(true, None).is_none(), "no manager yet");
}

#[test]
fn the_manager_stop_that_labs_off_runs_stops_the_sandbox() {
    // `apply_settings(off)` unwires, then asks the manager to stop; the fake records it.
    let fake = Arc::new(FakeSandbox::new());
    let manager = manager_of(fake.clone(), Arc::new(NoEvents));
    manager.start().unwrap();
    assert!(fake.is_running());
    manager.stop().unwrap();
    assert!(!fake.is_running());
    assert_eq!(fake.calls(), vec![Recorded::Start, Recorded::Stop]);
}
