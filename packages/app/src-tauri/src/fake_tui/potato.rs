//! The Potato Bot end-to-end rig (`tests/fake_tui_potato.rs`): the real Potato Bot service with the real
//! sidecar, starting a real Node process that runs `packages/potato-brain/test/fake-brain.mjs` (no model,
//! no key, no network), behind the real bridge `handle_connection` (gate, brain routes, router) listening on
//! a port the OS picks. Nothing here binds 17321, 17322 or 9333, writes a port file, or touches the person's
//! app data: the harness already roots app data in a temp folder.
//!
//! What is fake, and nothing else: the brain. It speaks channel protocol v1 and answers "Fish heard: <text>".
//!
//! Two seams only: `bridge::serve_for_e2e` (the bridge's own connection handler on a free port) and
//! `potato::live::start` (what `lib.rs` calls in `setup`, which the windowless test app does not run). Dev only.

use super::world::World;
use crate::companion::model::CompanionProfile;
use crate::potato::live;
use crate::potato::service::Service;
use crate::potato::store::{BrainKind, Settings};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub struct PotatoRig {
    service: Arc<Service>,
    port: u16,
}

/// The fake brain script of the brain package.
fn fake_brain() -> PathBuf {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../potato-brain/test/fake-brain.mjs");
    let full = path.canonicalize().expect("packages/potato-brain/test/fake-brain.mjs");
    // Node does not take the `\?\` form canonicalize gives on Windows.
    PathBuf::from(full.to_string_lossy().trim_start_matches("\\\\?\\"))
}

fn companion(id: &str, name: &str, folder: &std::path::Path) -> CompanionProfile {
    CompanionProfile {
        id: id.into(),
        name: name.into(),
        avatar: "mochi".into(),
        persona: "calm".into(),
        agent_type: "fake_claude".into(),
        launch_args: vec![],
        account: None,
        folder: folder.to_string_lossy().into_owned(),
        auto_start: false,
        memory_review: true,
        push_replies: false,
        share_today: false,
        nap_at: None,
        checkins: vec![],
        previous_agent: None,
        until_ms: None,
        created_ms: 1,
        archived: false,
        pane_ids: vec![],
    }
}

impl PotatoRig {
    /// Starts the service, the bridge listener and the fake brain's settings once; later calls find them.
    pub fn start() -> &'static PotatoRig {
        static RIG: OnceLock<PotatoRig> = OnceLock::new();
        RIG.get_or_init(|| {
            let (app, registry) = World::get().handles();
            std::env::set_var("PM_POTATO_BRAIN", fake_brain());
            let port = crate::bridge::serve_for_e2e(registry.clone(), app.clone()).expect("a bridge listener on a free port");
            live::set_bridge_port(port);
            // The companion the bot is made from.
            let data = crate::app_paths::app_data_dir();
            let home = data.join("companions-home").join("nimbus");
            std::fs::create_dir_all(&home).expect("the companion home");
            crate::companion::store::save(&data, &[companion("nimbus", "Nimbus", &home)]).expect("companions.json");
            let service = live::start(Some(app), Some(registry)).expect("the Potato Bot service starts once per process");
            // No key is stored: the Go-plan brain needs none, and the real credential store is not touched.
            service.set_settings(Settings { brain: BrainKind::Go, ..Settings::default() }).expect("settings");
            live::apply_settings(&json!({ "potato_bot_enabled": true }));
            PotatoRig { service, port }
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn add_bot(&self, id: &str) -> Result<(), String> {
        match self.service.add_bot(id) {
            Ok(_) => Ok(()),
            Err(e) if e.code == "POTATO_BOT_EXISTS" => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// One message from the person. The outcome, as the page would see it.
    pub fn send(&self, bot: &str, text: &str) -> Result<Value, String> {
        self.service.send(bot, text).map(|o| serde_json::to_value(o).unwrap_or_default()).map_err(|e| e.to_string())
    }

    /// The bot's thread as the page reads it.
    pub fn thread(&self, bot: &str) -> Vec<Value> {
        self.service.messages(bot, 500).map(|m| m.iter().filter_map(|m| serde_json::to_value(m).ok()).collect()).unwrap_or_default()
    }

    pub fn state(&self) -> Value {
        self.service.state().map(|s| serde_json::to_value(s).unwrap_or_default()).unwrap_or_default()
    }

    pub fn brain_pid(&self, bot: &str) -> Option<u32> {
        self.service.brain_pid(bot)
    }

    pub fn log(&self, bot: &str) -> Vec<String> {
        self.service.log_lines(bot)
    }

    pub fn stop(&self, bot: &str) {
        self.service.stop_bot(bot);
    }

    pub fn stop_all(&self) {
        self.service.stop_all(std::time::Duration::from_secs(2));
    }

    pub fn set_enabled(&self, on: bool) {
        live::apply_settings(&json!({ "potato_bot_enabled": on }));
    }

    pub fn start_again(&self, bot: &str) -> Result<(), String> {
        self.service.start_bot(bot).map_err(|e| e.to_string())
    }
}
