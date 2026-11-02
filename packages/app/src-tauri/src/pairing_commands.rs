//! "Pair my computers": the Tauri commands and the app-wide service behind them (Labs `desk_enabled`).
//!
//! Desktop window only. Nothing in `bridge/`, `mcp_shim/`, `tool_registry/`, the phone routes or the MCP tools
//! mentions this file (`p2p/guard_tests.rs`), so no agent and no phone can show a code, enter one, or answer the
//! "wants to pair" sheet. Every command says `FEATURE_OFF` while Labs is off.
//!
//! `pair_view` -> `{hosting, pending, joining, message, locked_until, tampered, devices}`; the same value arrives as
//! the event `pair://changed` whenever it changes. `pair_host_start`, `pair_host_cancel`, `pair_join(target, code)`,
//! `pair_answer(id, allow)`, `pair_nearby`, `trust_set_permission(node, capability, on)`, `trust_remove(node)`.
//!
//! The trust list is the signed file `p2p/trust.json` in the app data folder: public keys and names, no secrets.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::desk::link::{DeskLink, FileLinkStore, LinkView};
use crate::p2p::capability::Registry;
use crate::p2p::discovery::{Discovery, MdnsLan, NoDiscovery};
use crate::p2p::endpoint::NodeId;
use crate::p2p::iroh_net::IrohFactory;
use crate::p2p::pairing::{PairError, PairManager, SystemClock};
use crate::p2p::trust::{KeyStoreSigner, TrustList, TrustStorage};
use crate::p2p::P2p;
use crate::team::keystore::KeyStore;

const FEATURE_OFF: &str = "FEATURE_OFF: My computers is switched off in Labs";

/// The signed list on disk, written whole through a temporary file.
struct FileTrustStorage {
    path: PathBuf,
}

impl TrustStorage for FileTrustStorage {
    fn load(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.path).ok()
    }

    fn save(&self, bytes: &[u8]) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

struct Service {
    p2p: Arc<P2p>,
    mgr: Arc<PairManager>,
    link: Arc<DeskLink>,
}

static LABS: AtomicBool = AtomicBool::new(false);
static SERVICE: OnceLock<Result<Service, String>> = OnceLock::new();

fn computer_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "This computer".into())
}

/// Built on first use, never at app start: no key is read and no socket is opened before the person opens the page.
fn service() -> Result<&'static Service, String> {
    let built = SERVICE.get_or_init(|| {
        let keys = KeyStore::app();
        let storage = Arc::new(FileTrustStorage { path: crate::app_paths::app_data_dir().join("p2p").join("trust.json") });
        let trust = TrustList::open(storage, Arc::new(KeyStoreSigner::new(keys.clone())));
        let p2p = Arc::new(P2p::new(keys.clone(), Arc::new(IrohFactory), Arc::new(Registry::new()), trust.clone()));
        p2p.apply_settings(&json!({ "desk_enabled": LABS.load(Ordering::SeqCst) }));
        let mgr = PairManager::new(p2p.clone(), trust.clone(), keys, Arc::new(SystemClock), mdns_or_none(), &computer_name())
            .map_err(|e| e.to_string())?;
        let store = Arc::new(FileLinkStore { path: crate::app_paths::app_data_dir().join("desk").join("link.json") });
        let link = DeskLink::new(p2p.clone(), trust, crate::desk::link::remote::default_remote(), store, &computer_name()).map_err(|e| e.to_string())?;
        link.apply_settings(LABS.load(Ordering::SeqCst));
        Ok(Service { p2p, mgr, link })
    });
    built.as_ref().map_err(|e| e.clone())
}

fn ready(app: &AppHandle) -> Result<&'static Service, String> {
    if !LABS.load(Ordering::SeqCst) {
        return Err(FEATURE_OFF.into());
    }
    let s = service()?;
    static ATTACH: std::sync::Once = std::sync::Once::new();
    let app = app.clone();
    let mgr = s.mgr.clone();
    ATTACH.call_once(move || {
        mgr.set_notifier(Arc::new(move |view: &Value| {
            let _ = app.emit("pair://changed", view);
        }));
    });
    Ok(s)
}

/// Called with the effective public settings whenever they change. Labs off closes the endpoint.
pub fn apply_settings(public: &Value) {
    LABS.store(crate::p2p::labs_on(public), Ordering::SeqCst);
    if let Some(Ok(s)) = SERVICE.get() {
        s.p2p.apply_settings(public);
        s.link.apply_settings(crate::p2p::labs_on(public));
        if !crate::p2p::labs_on(public) {
            s.mgr.cancel_hosting();
        }
    }
}

/// App quit.
pub fn shutdown() {
    if let Some(Ok(s)) = SERVICE.get() {
        // The pointer comes home first.
        s.link.shutdown();
        s.mgr.cancel_hosting();
        s.p2p.stop();
    }
}

/// The desk link for the page's commands: built on first use, listening for the page's changes, started when
/// Labs is on. Same rules as `ready`.
pub(crate) fn link_ready(app: &AppHandle) -> Result<Arc<DeskLink>, String> {
    let s = ready(app)?;
    static ATTACH: std::sync::Once = std::sync::Once::new();
    let app = app.clone();
    let link = s.link.clone();
    ATTACH.call_once(move || {
        let layout_app = app.clone();
        link.set_notifier(Arc::new(move |view: &LinkView| {
            let _ = app.emit("desk://link", view);
        }));
        link.set_layout_notifier(Arc::new(move |view: &crate::desk::link::LayoutView| {
            let _ = layout_app.emit("desk://layout", view);
        }));
    });
    s.link.start();
    Ok(s.link.clone())
}

pub(crate) fn node_of(hex: &str) -> Result<NodeId, String> {
    if hex.len() != 64 || !hex.is_ascii() {
        return Err("Not a computer.".into());
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| "Not a computer.".to_string())?;
    }
    Ok(NodeId(out))
}

fn text(e: PairError) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn pair_view(app: AppHandle) -> Result<Value, String> {
    Ok(ready(&app)?.mgr.view())
}

#[tauri::command]
pub async fn pair_host_start(app: AppHandle) -> Result<Value, String> {
    ready(&app)?.mgr.start_hosting().await.map_err(text)
}

#[tauri::command]
pub async fn pair_host_cancel(app: AppHandle) -> Result<Value, String> {
    let s = ready(&app)?;
    s.mgr.cancel_hosting();
    Ok(s.mgr.view())
}

#[tauri::command]
pub async fn pair_join(app: AppHandle, target: String, code: String) -> Result<Value, String> {
    let s = ready(&app)?;
    s.mgr.join(&target, &code).await.map_err(text)?;
    Ok(s.mgr.view())
}

/// The person's answer to the "wants to pair" sheet. Only this window can send it.
#[tauri::command]
pub async fn pair_answer(app: AppHandle, id: u64, allow: bool) -> Result<Value, String> {
    let s = ready(&app)?;
    s.mgr.answer(id, allow).map_err(text)?;
    Ok(s.mgr.view())
}

#[tauri::command]
pub async fn pair_nearby(app: AppHandle) -> Result<Value, String> {
    Ok(ready(&app)?.mgr.nearby())
}

#[tauri::command]
pub async fn trust_set_permission(app: AppHandle, node: String, capability: String, on: bool) -> Result<Value, String> {
    let s = ready(&app)?;
    s.mgr.set_permission(&node_of(&node)?, &capability, on).map_err(text)?;
    // A switch turned off closes the open link now, not at the next upkeep.
    s.link.permission_changed();
    Ok(s.mgr.view())
}

#[tauri::command]
pub async fn trust_remove(app: AppHandle, node: String) -> Result<Value, String> {
    let s = ready(&app)?;
    s.mgr.remove(&node_of(&node)?).map_err(text)?;
    s.link.permission_changed();
    Ok(s.mgr.view())
}

/// The LAN announcer, or nobody when the network radio cannot start ("Add by address" still works).
fn mdns_or_none() -> Arc<dyn Discovery> {
    match MdnsLan::new() {
        Ok(m) => Arc::new(m),
        Err(_) => Arc::new(NoDiscovery),
    }
}
