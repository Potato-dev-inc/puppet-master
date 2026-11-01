//! "Remove chat" for any agent (Settings > Agents > How to run). Only the app's own webview calls it, behind the
//! Labs switch like the rest of the chat lane; the bridge has no route for it.
//!
//! `acp_chat_remove(agent, {remove_addon})` does, in this order, and every step is safe to repeat:
//!
//! 1. stops the agent's open chats (the adapter and everything under it, open permission cards expire);
//! 2. its live saved conversations stay as records, closed, with the notice "Chat removed...";
//! 3. the agent's lane goes back to Terminal;
//! 4. the person's chat overlay entry is removed, if there is one;
//! 5. with `remove_addon`, the chat add-on files of that one agent are deleted from the app data folder.
//!
//! The add-on is only ever the folder this app installed for the agent under `<app data>/acp/adapters/`: a plain
//! folder directly inside that folder, never a link, never a path the agent's definition could point elsewhere.
//! Links inside it are removed as links, never followed. A failure to delete the files is reported in the answer
//! and does not undo the rest.

use super::*;
use crate::acp::config::{HostEnv, RealHost};
use crate::acp::conversations::Conversations;
use crate::acp::install;
use crate::acp::lane::{LaneStore, RunLane};
use crate::acp::overlay::{OverlayItem, OverlayStore};
use serde_json::json;
use std::path::Path;

/// The most entries the size walk looks at (a node_modules folder has thousands, never millions).
const WALK_LIMIT: usize = 200_000;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RemoveOpts {
    /// Also delete the agent's chat add-on files.
    pub remove_addon: bool,
}

fn labs_gate() -> Result<(), String> {
    if crate::acp::chat_enabled() {
        Ok(())
    } else {
        Err("FEATURE_OFF: chat is switched off in Labs".into())
    }
}

fn is_plain_folder_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// The folder (directly under the adapters folder) that holds this agent's add-on, whether or not it exists yet.
/// An overlay entry installed from the registry names its own; otherwise it is the folder the agent's own table
/// installs into.
pub(crate) fn addon_dir_of(host: &dyn HostEnv, overlay: &OverlayStore, agent: &str) -> Option<PathBuf> {
    let id = agent.trim().to_ascii_lowercase();
    if let Some(OverlayItem::Registry(entry)) = overlay.stored_item(&id) {
        let folder = entry.installed_entry.split(['/', '\\']).next().unwrap_or("").to_string();
        return is_plain_folder_name(&folder).then(|| host.adapters_dir().join(folder));
    }
    let profile = crate::tui::registry::registry().find(&id)?;
    let table = profile.acp_table()?;
    install::plan(&table, host).ok().map(|plan| plan.dir)
}

/// Whether `dir` is a plain (not linked) folder that sits directly in the adapters folder.
fn inside_adapters(host: &dyn HostEnv, dir: &Path) -> Result<(), String> {
    let adapters = host.adapters_dir();
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if dir.parent() != Some(adapters.as_path()) || !is_plain_folder_name(name) {
        return Err("the add-on is not in the app's add-on folder".into());
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("the add-on folder is a link, not a folder, so it was left alone".into());
    }
    // Resolved, it is still where it should be.
    let real = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
    let real_adapters = std::fs::canonicalize(&adapters).map_err(|e| e.to_string())?;
    if real.parent() != Some(real_adapters.as_path()) {
        return Err("the add-on folder resolves outside the app's add-on folder, so it was left alone".into());
    }
    Ok(())
}

/// What an add-on takes on disk, in whole megabytes (at least 1), or `None` when it is not installed.
pub(crate) fn addon_size_mb(dir: &Path) -> Option<u32> {
    let meta = std::fs::symlink_metadata(dir).ok()?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return None;
    }
    let mut bytes = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&folder) else { continue };
        for entry in read.flatten() {
            seen += 1;
            if seen > WALK_LIMIT {
                break;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                bytes += meta.len();
            }
        }
    }
    Some(((bytes + (1 << 20) - 1) >> 20).max(1) as u32)
}

/// Deletes the folder and everything in it. A link inside is removed as a link; nothing is followed out of the
/// folder. Called only after [`inside_adapters`].
fn delete_tree(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            // A link to a folder is removed with `remove_dir` on Windows and `remove_file` elsewhere; try both.
            std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir(&path))?;
        } else if meta.is_dir() {
            delete_tree(&path)?;
        } else {
            remove_file_forced(&path)?;
        }
    }
    std::fs::remove_dir(dir)
}

fn remove_file_forced(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(first) => {
            // A read-only file (npm leaves some) blocks deletion on Windows.
            let mut perms = std::fs::metadata(path)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            std::fs::set_permissions(path, perms)?;
            std::fs::remove_file(path).map_err(|_| first)
        }
    }
}

/// What a removal did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Removal {
    /// Open chats stopped.
    pub chats_stopped: usize,
    /// Saved conversations marked "chat removed".
    pub conversations: usize,
    /// The overlay entry was there and is gone.
    pub overlay_removed: bool,
    /// The add-on files were deleted (`Some(true)`), were not there (`Some(false)`), or could not be (`None` + error).
    pub addon_deleted: Option<bool>,
    pub addon_error: Option<String>,
}

pub(crate) struct Stores<'a> {
    pub lanes: &'a LaneStore,
    pub overlay: &'a OverlayStore,
    pub host: &'a dyn HostEnv,
    pub conversations: &'a Conversations,
}

/// The whole removal over given stores. `stop_chats(agent)` stops the open chats and says how many (live app: the
/// chat registry; tests: a table).
pub(crate) fn remove_in(stores: &Stores, agent: &str, remove_addon: bool, stop_chats: &dyn Fn(&str) -> usize) -> Result<Removal, String> {
    let id = agent.trim().to_ascii_lowercase();
    if id.is_empty() {
        return Err("INVALID_ARGUMENT: agent is required".into());
    }
    let mut out = Removal { chats_stopped: stop_chats(&id), ..Removal::default() };
    out.conversations = stores.conversations.chat_removed(&id).len();
    // Every lane (Acp, terminal chat) goes back to Terminal; a terminal-chat lane is also reset by the shared overlay removal below.
    stores.lanes.set(&id, RunLane::Terminal)?;
    // The add-on folder is read before the overlay entry goes: a registry add-on is named by that entry.
    let addon = remove_addon.then(|| addon_dir_of(stores.host, stores.overlay, &id)).flatten();
    out.overlay_removed = super::acp_overlay::remove_in(stores.overlay, stores.lanes, &id)?;
    if remove_addon {
        match addon {
            None => out.addon_deleted = Some(false),
            Some(dir) if !dir.exists() && std::fs::symlink_metadata(&dir).is_err() => out.addon_deleted = Some(false),
            Some(dir) => match inside_adapters(stores.host, &dir).and_then(|()| delete_tree(&dir).map_err(|e| format!("{e}. If a chat with this agent is still open, close it first"))) {
                Ok(()) => out.addon_deleted = Some(true),
                Err(why) => out.addon_error = Some(why),
            },
        }
    }
    Ok(out)
}

fn report(removal: &Removal) -> Value {
    json!({
        "chats_stopped": removal.chats_stopped,
        "conversations": removal.conversations,
        "overlay_removed": removal.overlay_removed,
        "addon_deleted": removal.addon_deleted,
        "addon_error": removal.addon_error,
    })
}

/// The lane view with the report of what was done under `removal`.
fn view_with(removal: &Removal) -> Value {
    let mut view = super::acp_lane::view();
    view["removal"] = report(removal);
    view
}

/// Removes Chat for one agent (see the module doc). Returns the lane view, plus `removal` with what happened.
/// Safe to call again; an unknown or already removed agent is not an error.
#[tauri::command]
pub async fn acp_chat_remove(app: AppHandle, agent: String, opts: Option<RemoveOpts>) -> Result<Value, String> {
    labs_gate()?;
    let remove_addon = opts.unwrap_or_default().remove_addon;
    let removal = tauri::async_runtime::spawn_blocking(move || {
        let lanes = LaneStore::in_app_data();
        let overlay = OverlayStore::in_app_data();
        let stores = Stores { lanes: &lanes, overlay: &overlay, host: &RealHost, conversations: crate::acp::conversations::global() };
        remove_in(&stores, &agent, remove_addon, &|id| crate::acp::chat::close_agent(id))
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))??;
    let _ = app.emit("acp-conversations-changed", ());
    let _ = app.emit("pty://panes-changed", ());
    Ok(view_with(&removal))
}

/// For the lane rows: `{installed, size_mb}` of the add-on that Remove could delete, or `None` when there is none.
pub(crate) fn addon_info(host: &dyn HostEnv, overlay: &OverlayStore, agent: &str) -> Option<Value> {
    let dir = addon_dir_of(host, overlay, agent)?;
    let size = addon_size_mb(&dir)?;
    Some(json!({ "installed": true, "size_mb": size }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::config;
    use crate::acp::overlay::{OverlayEntry, RegistryEntry};
    use crate::acp::record::RecordStore;
    use crate::acp::supervisor::{Supervisor, SupervisorConfig};
    use std::sync::Arc;

    struct Host(PathBuf);
    impl HostEnv for Host {
        fn var(&self, name: &str) -> Option<std::ffi::OsString> {
            std::env::var_os(name)
        }
        fn which(&self, _: &str) -> Option<PathBuf> {
            None
        }
        fn adapters_dir(&self) -> PathBuf {
            self.0.join("acp").join("adapters")
        }
        fn homes_dir(&self) -> PathBuf {
            self.0.join("acp").join("homes")
        }
    }

    struct Rig {
        dir: PathBuf,
        lanes: LaneStore,
        overlay: OverlayStore,
        host: Host,
        conversations: Conversations,
    }

    impl Rig {
        fn new() -> Rig {
            let dir = std::env::temp_dir().join(format!("pm-acp-remove-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(dir.join("acp").join("adapters")).unwrap();
            let host = Host(dir.clone());
            let conversations = Conversations::new(
                RecordStore::at(dir.join("conversations.json")),
                Arc::new(Supervisor::new(SupervisorConfig::default())),
                Arc::new(Host(dir.clone())),
                Arc::new(RegisteredNone),
            );
            Rig { lanes: LaneStore::at(dir.join("agent_lane.json")), overlay: OverlayStore::at(dir.join("acp_overlay.json")), host, conversations, dir }
        }
        fn stores(&self) -> Stores<'_> {
            Stores { lanes: &self.lanes, overlay: &self.overlay, host: &self.host, conversations: &self.conversations }
        }
        fn registry_entry(&self, folder: &str) -> RegistryEntry {
            RegistryEntry { registry_id: "demo".into(), version: "1.0.0".into(), installed_entry: format!("{folder}/node_modules/demo/index.js"), entry_sha256: "0".repeat(64), ..RegistryEntry::default() }
        }
        fn install_files(&self, folder: &str) -> PathBuf {
            let dir = self.host.adapters_dir().join(folder);
            std::fs::create_dir_all(dir.join("node_modules").join("demo")).unwrap();
            std::fs::write(dir.join("node_modules").join("demo").join("index.js"), vec![b'x'; 3 << 20]).unwrap();
            dir
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    struct RegisteredNone;
    impl crate::acp::conversations::AgentSource for RegisteredNone {
        fn config(&self, _: &str) -> Option<Arc<config::AgentConfig>> {
            None
        }
    }

    fn a_registered_agent() -> String {
        crate::tui::registry::registry().ordered().iter().map(|p| p.id().to_string()).find(|id| !crate::tui::is_shell(id) && crate::tui::registry::registry().find(id).is_some_and(|p| p.acp_table().is_none())).expect("an agent with no table of its own")
    }

    #[test]
    fn removing_resets_the_lane_and_the_overlay_and_is_safe_to_repeat() {
        let rig = Rig::new();
        let agent = a_registered_agent();
        rig.overlay.put(&agent, &OverlayEntry { command: "adapter".into(), args: vec!["acp".into()], ..Default::default() }).unwrap();
        rig.lanes.set(&agent, RunLane::Acp).unwrap();
        let first = remove_in(&rig.stores(), &agent, false, &|_| 2).unwrap();
        assert_eq!((first.chats_stopped, first.overlay_removed, first.addon_deleted), (2, true, None));
        assert_eq!(rig.lanes.lane_for(&agent), RunLane::Terminal);
        assert!(rig.overlay.get(&agent).is_none());
        let again = remove_in(&rig.stores(), &agent.to_uppercase(), true, &|_| 0).unwrap();
        assert_eq!((again.chats_stopped, again.overlay_removed, again.addon_deleted, again.addon_error), (0, false, Some(false), None));
        assert!(remove_in(&rig.stores(), "  ", false, &|_| 0).unwrap_err().contains("INVALID_ARGUMENT"));
    }

    #[test]
    fn removing_a_terminal_chat_lane_resets_it_to_terminal() {
        let rig = Rig::new();
        let agent = a_registered_agent();
        rig.lanes.set(&agent, RunLane::TerminalChat).unwrap();
        let done = remove_in(&rig.stores(), &agent, false, &|_| 0).unwrap();
        assert!(!done.overlay_removed, "a terminal chat has no overlay entry");
        assert_eq!(rig.lanes.lane_for(&agent), RunLane::Terminal);
    }

    #[test]
    fn the_add_on_is_deleted_only_when_asked() {
        let rig = Rig::new();
        let agent = a_registered_agent();
        let files = rig.install_files("reg-demo");
        rig.overlay.put_item(&agent, &OverlayItem::Registry(rig.registry_entry("reg-demo"))).unwrap();
        assert_eq!(addon_size_mb(&files), Some(3));
        assert_eq!(addon_info(&rig.host, &rig.overlay, &agent).unwrap()["size_mb"], json!(3));

        let kept = remove_in(&rig.stores(), &agent, false, &|_| 0).unwrap();
        assert_eq!(kept.addon_deleted, None);
        assert!(files.exists(), "the add-on stays unless the person ticks the box");

        rig.overlay.put_item(&agent, &OverlayItem::Registry(rig.registry_entry("reg-demo"))).unwrap();
        let gone = remove_in(&rig.stores(), &agent, true, &|_| 0).unwrap();
        assert_eq!((gone.addon_deleted, gone.overlay_removed), (Some(true), true));
        assert!(!files.exists());
        assert!(rig.host.adapters_dir().exists(), "only that agent's folder goes");
        assert_eq!(addon_info(&rig.host, &rig.overlay, &agent), None);
    }

    #[test]
    fn a_folder_name_that_points_elsewhere_is_never_deleted() {
        let rig = Rig::new();
        let agent = a_registered_agent();
        let outside = rig.dir.join("precious");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "mine").unwrap();
        for entry in ["../../precious/x.js", "..\\..\\precious\\x.js", "/etc/x.js", "C:\\Windows\\x.js", "./x.js", "x.js"] {
            let mut e = rig.registry_entry("reg-demo");
            e.installed_entry = entry.to_string();
            rig.overlay.put_item(&agent, &OverlayItem::Registry(e)).unwrap();
            let done = remove_in(&rig.stores(), &agent, true, &|_| 0).unwrap();
            assert_ne!(done.addon_deleted, Some(true), "{entry}");
            assert!(outside.join("keep.txt").exists(), "{entry}");
        }
    }

    #[test]
    fn a_link_inside_the_add_on_is_removed_as_a_link_and_never_followed() {
        let rig = Rig::new();
        let agent = a_registered_agent();
        let files = rig.install_files("reg-demo");
        let outside = rig.dir.join("precious");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "mine").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, files.join("link")).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, files.join("link")).is_ok();
        rig.overlay.put_item(&agent, &OverlayItem::Registry(rig.registry_entry("reg-demo"))).unwrap();
        let done = remove_in(&rig.stores(), &agent, true, &|_| 0).unwrap();
        assert_eq!(done.addon_deleted, Some(true), "{done:?}");
        assert!(outside.join("keep.txt").exists(), "what the link pointed at is untouched (link made: {linked})");
    }

    #[test]
    fn a_folder_that_is_itself_a_link_is_left_alone_with_a_reason() {
        let rig = Rig::new();
        let agent = a_registered_agent();
        let outside = rig.dir.join("precious");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "mine").unwrap();
        let link = rig.host.adapters_dir().join("reg-demo");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, &link).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &link).is_ok();
        if !linked {
            return; // this account may not make links; the walker test above covers the rule
        }
        rig.overlay.put_item(&agent, &OverlayItem::Registry(rig.registry_entry("reg-demo"))).unwrap();
        let done = remove_in(&rig.stores(), &agent, true, &|_| 0).unwrap();
        assert_eq!(done.addon_deleted, None);
        assert!(done.addon_error.unwrap().contains("link"));
        assert!(outside.join("keep.txt").exists());
    }

    #[test]
    fn the_commands_need_the_labs_switch() {
        if !crate::acp::chat_enabled() {
            assert!(labs_gate().unwrap_err().starts_with("FEATURE_OFF"));
        }
    }
}
