//! Preview windows, their guards, the pick pull loop and the after-pick pipeline (PF-10).
//!
//! A preview is a Tauri window labelled `preview-<n>` that loads the app's loopback URL directly. It is in
//! no capability, so the page has no way to call Puppet Master (SPEC n32 S3). Rust pulls picks out of the
//! page: a thread asks the picker for its queue every 250 ms while the window is focused or armed, else
//! every 2 s. The picker sits under a random, hidden property name, injected only into these windows.
//!
//! What happens after a pick: clean it (`pick::sanitize`), take a native screenshot, crop and save it in
//! the dev server pane's folder, find the source file, add the pick to the session and show the Point box
//! next to the element. Nothing here sends anything; sending is the Point box's Send button.

use super::box_window::{self, Rect4};
use super::origin::{self, Nav, OwnPorts, PreviewOrigin};
use super::pick::{self, PickInput};
use super::session::{PointState, SESSION};
use super::target::{self, TargetFacts};
use super::{shot, source, Pick, Shot, KEEP_SHOTS};
use crate::pane_outbox::PaneKind;
use crate::pty::PaneInfo;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

/// Every preview window's label starts with this. No capability may match it (pinned by a test).
pub const PREVIEW_PREFIX: &str = "preview-";
/// The picker script. `__PM_ORIGIN__` and `__PM_KEY__` are replaced by `picker_script`.
const PICKER_JS: &str = include_str!("picker.js");
/// How often the pull thread asks the page for a pick while the window is in use, and otherwise.
const FAST_POLL: Duration = Duration::from_millis(250);
const SLOW_POLL: Duration = Duration::from_secs(2);
/// How long the pull thread waits for the page to answer one question.
const TAKE_TIMEOUT: Duration = Duration::from_secs(2);
/// How long the native screenshot may take.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(3);
/// "Point" armed from Puppet Master keeps the fast poll for this long.
const ARMED_FAST_MS: u64 = 60_000;
/// The page's answer is cut off beyond this many bytes (a page cannot flood the app).
const MAX_TAKE_BYTES: usize = 200_000;
/// The crop around the element, in CSS px.
const CROP_PAD: f64 = 24.0;
/// Preview size as a share of the cockpit's monitor.
const WIDTH_SHARE: f64 = 0.60;
const HEIGHT_SHARE: f64 = 0.85;

/// What the open command returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewInfo {
    pub preview_id: String,
    pub url: String,
    pub title: String,
}

/// One open preview window.
#[derive(Clone)]
struct Preview {
    id: String,
    n: u32,
    origin: PreviewOrigin,
    /// The name of the picker's hidden property in the page.
    key: String,
    /// The pane that runs the dev server (the "opener") and its folder.
    opener: String,
    cwd: String,
    /// Fast polling until this time (ms), set when the person arms Point from Puppet Master.
    armed_until: Arc<AtomicU64>,
    gone: Arc<AtomicBool>,
    warned: Arc<AtomicBool>,
}

static PREVIEWS: Lazy<Mutex<Vec<Preview>>> = Lazy::new(|| Mutex::new(Vec::new()));
static NEXT_PREVIEW: AtomicU32 = AtomicU32::new(1);

// ---------------------------------------------------------------- pure helpers

/// `preview-3`.
pub fn preview_label(n: u32) -> String {
    format!("{PREVIEW_PREFIX}{n}")
}

/// `Preview · localhost:5173`.
pub fn preview_title(origin: &PreviewOrigin) -> String {
    format!("Preview \u{b7} {}:{}", origin.host, origin.port)
}

/// A new hidden property name for one window: `__pm_` and 16 random hex digits.
pub fn new_key() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("__pm_{}", &id[..16])
}

/// The picker with its two placeholders filled in as JSON strings (so no quote can break out).
pub fn picker_script(origin: &PreviewOrigin, key: &str) -> String {
    let json = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    PICKER_JS.replace("__PM_ORIGIN__", &json(&origin.as_str())).replace("__PM_KEY__", &json(key))
}

/// The JS that takes one queued pick out of the page. Evaluates to a JSON string or null.
fn take_script(key: &str) -> String {
    call_script(key, "take", "")
}

/// A call into the picker's frozen API, safe when the picker is absent (another origin).
fn call_script(key: &str, method: &str, arg: &str) -> String {
    let key = serde_json::to_string(key).unwrap_or_default();
    format!("(function(){{try{{var a=window[{key}];return a&&a.{method}?a.{method}({arg}):null}}catch(e){{return null}}}})()")
}

/// What `eval_with_callback` gives back for the take script: the JSON of the page's answer. Only a
/// string counts (the picker's JSON text); null, other types and oversize answers are nothing.
pub fn parse_take(raw: &str) -> Option<String> {
    if raw.len() > MAX_TAKE_BYTES {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(raw).ok()? {
        serde_json::Value::String(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// Where a new preview goes: a share of the cockpit's monitor, beside the cockpit (right of it when
/// there is room, else left, else flush right), cascaded by `slot`. All values are physical pixels.
pub fn preview_bounds(monitor: Rect4, cockpit: Rect4, slot: usize) -> Rect4 {
    let (mx, my, mw, mh) = monitor;
    let w = (f64::from(mw) * WIDTH_SHARE) as u32;
    let h = (f64::from(mh) * HEIGHT_SHARE) as u32;
    let mright = mx + mw as i32;
    let mut x = cockpit.0 + cockpit.2 as i32;
    if x + w as i32 > mright {
        x = cockpit.0 - w as i32;
    }
    if x < mx {
        x = mright - w as i32;
    }
    let y = my + ((mh - h) / 2) as i32;
    let step = (crate::app_windows::CASCADE_STEP as i32) * (slot % crate::app_windows::CASCADE_SLOTS) as i32;
    let x = (x + step).min(mright - w as i32).max(mx);
    let y = (y + step).min(my + mh as i32 - h as i32).max(my);
    (x, y, w, h)
}

/// True for a file the pipeline made itself: `.../.puppet-master/shots/pick-*.png`. Only these are
/// ever deleted when a pick is dropped.
pub fn is_own_shot(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let in_shots = path.parent().is_some_and(|dir| {
        dir.file_name().is_some_and(|n| n == "shots") && dir.parent().and_then(|p| p.file_name()).is_some_and(|n| n == ".puppet-master")
    });
    in_shots && name.starts_with("pick-") && name.ends_with(".png")
}

/// The screen rectangle (physical pixels) of an element, from its CSS rect and the window's inner box.
/// `scale` is physical pixels per CSS pixel.
pub fn screen_rect(inner: (i32, i32), rect: &super::Rect, scale: f64) -> Rect4 {
    let px = |v: f64| if v.is_finite() { (v * scale).round() as i32 } else { 0 };
    (inner.0 + px(rect.x), inner.1 + px(rect.y), px(rect.w).max(0) as u32, px(rect.h).max(0) as u32)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------------------------------------------------------------- opening

/// Opens the app at `port` of pane `pane_id` in a preview window (or focuses the one for the same
/// origin). The address comes only from what the pane printed or from its background jobs, and only
/// loopback addresses on ports that are not Puppet Master's own are allowed (S1, S4).
pub fn open(app: &AppHandle, pane_id: &str, port: u16, armed: bool) -> Result<PreviewInfo, String> {
    let pane = pane_infos(app).into_iter().find(|p| p.id == pane_id).ok_or("That pane is not open any more.")?;
    let (url, origin) = resolve(app, &pane, port)?;
    let title = preview_title(&origin);

    // The same origin opens once: focus it (and go to the address if the page left the origin).
    let existing = PREVIEWS.lock().iter().find(|p| p.origin == origin && !p.gone.load(Ordering::Relaxed)).cloned();
    if let Some(rec) = existing {
        if let Some(window) = app.get_webview_window(&rec.id) {
            if !window.url().map(|u| origin.matches(u.as_str())).unwrap_or(false) {
                if let Ok(target) = tauri::Url::parse(&url) {
                    let _ = window.navigate(target);
                }
            }
            focus(&window);
            if armed {
                arm(&rec, &window);
            }
            return Ok(PreviewInfo { preview_id: rec.id, url, title });
        }
    }

    let n = NEXT_PREVIEW.fetch_add(1, Ordering::Relaxed);
    let id = preview_label(n);
    let key = new_key();
    let parsed = tauri::Url::parse(&url).map_err(|_| origin::Refusal::BadUrl.message().to_string())?;

    let nav_app = app.clone();
    let nav_origin = origin.clone();
    let nav_id = id.clone();
    let win_app = app.clone();
    let win_id = id.clone();
    let mut builder = WebviewWindowBuilder::new(app, &id, WebviewUrl::External(parsed))
        .title(&title)
        .visible(false)
        .initialization_script(picker_script(&origin, &key))
        // Its own cookies and storage, apart from Puppet Master's pages (S13).
        .data_directory(crate::app_paths::app_data_dir().join("preview-web"))
        .on_navigation(move |next| match origin::nav(&nav_origin, next.as_str()) {
            Nav::Allow => true,
            Nav::Block { host } => {
                note_blocked(&nav_app, &nav_id, &host, next.as_str());
                false
            }
        })
        .on_new_window(move |next, _features| {
            let host = next.host_str().unwrap_or("an unknown address").to_string();
            note_blocked(&win_app, &win_id, &host, next.as_str());
            NewWindowResponse::Deny
        })
        .on_download(|_webview, _event| false);
    if let Some(port) = std::env::var("PM_PREVIEW_CDP_PORT").ok().and_then(|v| v.trim().parse::<u16>().ok()) {
        // Live checks only. Setting browser arguments replaces wry's defaults, so they are repeated.
        builder = builder.additional_browser_args(&format!(
            "--remote-debugging-port={port} --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"
        ));
    }
    let window = builder.build().map_err(|e| format!("The preview window did not open: {e}"))?;

    let rec = Preview {
        id: id.clone(),
        n,
        origin,
        key,
        opener: pane.id.clone(),
        cwd: pane.cwd.clone(),
        armed_until: Arc::new(AtomicU64::new(0)),
        gone: Arc::new(AtomicBool::new(false)),
        warned: Arc::new(AtomicBool::new(false)),
    };
    let slot = {
        let mut previews = PREVIEWS.lock();
        previews.push(rec.clone());
        previews.len() - 1
    };
    let (gone, closed_id) = (rec.gone.clone(), id.clone());
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            gone.store(true, Ordering::Relaxed);
            PREVIEWS.lock().retain(|p| p.id != closed_id);
        }
    });
    place(app, &window, slot);
    focus(&window);
    if armed {
        arm(&rec, &window);
    }
    let pull_app = app.clone();
    let pull_rec = rec.clone();
    std::thread::spawn(move || pull_loop(pull_app, pull_rec));
    Ok(PreviewInfo { preview_id: id, url, title })
}

/// The URL to open for `port` of this pane, and its checked origin.
fn resolve(app: &AppHandle, pane: &PaneInfo, port: u16) -> Result<(String, PreviewOrigin), String> {
    let previews = crate::pane_preview::snapshot().remove(&pane.id).unwrap_or_default();
    // The process list is only read when the pane did not print this port itself.
    let job_ports: Vec<u16> = if previews.iter().any(|p| p.port == port) {
        Vec::new()
    } else {
        let refs = [crate::pane_jobs::PaneRef { id: pane.id.clone(), agent_type: pane.agent_type.clone(), pid: pane.pid }];
        crate::pane_jobs::jobs_for_panes(&refs).remove(&pane.id).unwrap_or_default().into_iter().flat_map(|j| j.ports).collect()
    };
    let url = origin::resolve_open(port, &previews, &job_ports).map_err(|r| r.message().to_string())?;
    let dev_port = app.config().build.dev_url.as_ref().and_then(|u| u.port());
    let origin = origin::allowed(&url, &OwnPorts::current(dev_port)).map_err(|r| r.message().to_string())?;
    Ok((url, origin))
}

/// Size and position the new window on the cockpit's monitor, beside the cockpit, then show it.
fn place(app: &AppHandle, window: &WebviewWindow, slot: usize) {
    let cockpit = app.get_webview_window(crate::app_windows::COCKPIT_LABEL).or_else(|| app.get_webview_window("main"));
    let monitor = cockpit
        .as_ref()
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let monitor_rect = (monitor.position().x, monitor.position().y, monitor.size().width, monitor.size().height);
    let cockpit_rect = cockpit
        .as_ref()
        .and_then(|w| Some((w.outer_position().ok()?, w.outer_size().ok()?)))
        .map(|(p, s)| (p.x, p.y, s.width, s.height))
        .unwrap_or((monitor_rect.0, monitor_rect.1, 0, 0));
    let (x, y, w, h) = preview_bounds(monitor_rect, cockpit_rect, slot);
    let _ = window.set_size(PhysicalSize::new(w, h));
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

fn focus(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

fn arm(rec: &Preview, window: &WebviewWindow) {
    rec.armed_until.store(now_ms() + ARMED_FAST_MS, Ordering::Relaxed);
    let _ = window.eval(call_script(&rec.key, "arm", ""));
}

/// Focuses the newest preview window and arms the picker in it ("Point another", Alt+P from the box).
pub fn arm_newest(app: &AppHandle) -> Result<(), String> {
    let rec = PREVIEWS.lock().iter().max_by_key(|p| p.n).cloned().ok_or("Open a preview first.")?;
    let window = app.get_webview_window(&rec.id).ok_or("That preview is closed.")?;
    focus(&window);
    arm(&rec, &window);
    Ok(())
}

/// Shows a short note in the page (the picker's own toast, inside its closed shadow root). `preview_id`
/// empty means the newest preview.
pub fn toast(app: &AppHandle, preview_id: &str, text: &str) {
    let rec = {
        let previews = PREVIEWS.lock();
        if preview_id.is_empty() {
            previews.iter().max_by_key(|p| p.n).cloned()
        } else {
            previews.iter().find(|p| p.id == preview_id).cloned()
        }
    };
    let Some(rec) = rec else { return };
    if let Some(window) = app.get_webview_window(&rec.id) {
        let arg = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
        let _ = window.eval(call_script(&rec.key, "done", &arg));
    }
}

// ---------------------------------------------------------------- guards

/// A link or window the preview refused: keep the note, tell the page and the Point box.
fn note_blocked(app: &AppHandle, preview_id: &str, host: &str, url: &str) {
    let (app, preview_id, host, url) = (app.clone(), preview_id.to_string(), host.to_string(), url.to_string());
    // The navigation handler runs on the webview's thread; do the rest elsewhere.
    std::thread::spawn(move || {
        SESSION.lock().set_blocked(&host, &url);
        let state = current_state(&app);
        if let Some(text) = &state.notice {
            toast(&app, &preview_id, text);
        }
        if box_window::is_visible(&app) {
            box_window::emit_state(&app, &state);
        }
    });
}

// ---------------------------------------------------------------- the pull loop

fn pull_loop(app: AppHandle, rec: Preview) {
    loop {
        let Some(window) = app.get_webview_window(&rec.id) else { break };
        let fast = now_ms() < rec.armed_until.load(Ordering::Relaxed) || window.is_focused().unwrap_or(false);
        std::thread::sleep(if fast { FAST_POLL } else { SLOW_POLL });
        if rec.gone.load(Ordering::Relaxed) {
            break;
        }
        sweep(&app);
        // Only the app's own address may answer. If the page moved to another local app, ask nothing.
        if !window.url().map(|u| rec.origin.matches(u.as_str())).unwrap_or(false) {
            continue;
        }
        if let Some(raw) = take(&window, &rec.key) {
            after_pick(&app, &rec, &window, &raw);
        }
    }
    tracing::debug!(preview = %rec.id, "point: preview pull thread stopped");
}

/// Ends an expired session: deletes its shots and hides the box.
fn sweep(app: &AppHandle) {
    let expired = SESSION.lock().expire(now_ms());
    if let Some(shots) = expired {
        delete_shots(&shots);
        box_window::hide(app);
    }
}

/// Asks the page for one queued pick. None when there is none or the page does not answer.
fn take(window: &WebviewWindow, key: &str) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    window
        .eval_with_callback(take_script(key), move |answer| {
            let _ = tx.send(answer);
        })
        .ok()?;
    parse_take(&rx.recv_timeout(TAKE_TIMEOUT).ok()?)
}

// ---------------------------------------------------------------- after a pick

fn after_pick(app: &AppHandle, rec: &Preview, window: &WebviewWindow, raw: &str) {
    let facts = match serde_json::from_str::<PickInput>(raw) {
        Ok(input) => match pick::sanitize(input, &rec.origin) {
            Ok(facts) => facts,
            Err(refusal) => return warn_once(rec, refusal.message()),
        },
        Err(_) => return warn_once(rec, "The page sent a pick that could not be read."),
    };

    let seq = SESSION.lock().next_seq();
    let now = now_ms();
    let (saved, no_shot_reason) = match capture_png(window) {
        Err(reason) => (None, Some(reason)),
        Ok(png) => match save_crop(&png, &facts, Path::new(&rec.cwd), now, seq) {
            Ok(saved) => (Some(saved), None),
            Err(reason) => (None, Some(reason)),
        },
    };

    // The "To:" list and the folders a source file may live in: the dev server pane's and the default target's.
    let infos = pane_infos(app);
    let (targets, default) = target::targets(&target_facts(&infos), Some(&rec.opener), Some(&rec.cwd));
    let default_cwd = default.as_ref().and_then(|id| infos.iter().find(|p| &p.id == id)).map(|p| p.cwd.clone());
    let mut roots: Vec<&Path> = vec![Path::new(&rec.cwd)];
    roots.extend(default_cwd.as_deref().map(Path::new));
    let source = facts.source_hint.as_ref().and_then(|hint| source::resolve(hint, &roots)).map(|(_, found)| found);

    let screen = {
        let inner = window.inner_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
        let scale = match (window.inner_size(), facts.viewport.w) {
            (Ok(size), w) if w > 0.0 => f64::from(size.width) / w,
            _ => window.scale_factor().unwrap_or(1.0),
        };
        screen_rect(inner, &facts.rect, scale)
    };

    let pick = Pick {
        id: format!("pick-{seq}"),
        preview_id: rec.id.clone(),
        facts,
        source,
        shot: saved,
        no_shot_reason,
        created_ms: now,
    };
    let (added, count) = {
        let mut session = SESSION.lock();
        let added = session.add_pick(pick, now);
        (added, session.picks().len())
    };
    delete_shots(&added.dropped_shots);

    let state = current_state_with(targets, default);
    box_window::emit_state(app, &state);
    box_window::show_near(app, screen);
    box_window::emit_state(app, &state);
    rec.armed_until.store(0, Ordering::Relaxed);
    if count > 1 {
        toast(app, &rec.id, "Added. Point at another or write in the box.");
    }
}

/// The screenshot, cropped around the element and saved in the dev server pane's folder.
fn save_crop(png: &[u8], facts: &super::PickFacts, project: &Path, now: u64, seq: u32) -> Result<Shot, &'static str> {
    let (bytes, width, height) = shot::crop(png, facts.rect.clone(), facts.viewport.clone(), CROP_PAD)
        .map_err(|_| "The screenshot could not be cropped.")?;
    let (abs_path, rel_path) = shot::save(project, &bytes, now, seq).map_err(|_| "The screenshot could not be saved.")?;
    shot::prune(&shot::shots_dir(project), KEEP_SHOTS);
    Ok(Shot { abs_path, rel_path, width, height })
}

/// The native screenshot of the preview's viewport.
///
/// SHIM until PF-9 (`point/capture.rs`) is merged: then this body becomes
/// `super::capture::viewport_png(window, CAPTURE_TIMEOUT)` and nothing else changes.
fn capture_png(_window: &WebviewWindow) -> Result<Vec<u8>, &'static str> {
    let _ = CAPTURE_TIMEOUT;
    Err("Screenshots need Windows for now.")
}

fn warn_once(rec: &Preview, why: &str) {
    if !rec.warned.swap(true, Ordering::Relaxed) {
        tracing::warn!(preview = %rec.id, "point: a pick was refused: {why}");
    }
}

fn delete_shots(paths: &[PathBuf]) {
    for path in paths.iter().filter(|p| is_own_shot(p)) {
        let _ = std::fs::remove_file(path);
    }
}

// ---------------------------------------------------------------- state for the box

fn pane_infos(app: &AppHandle) -> Vec<PaneInfo> {
    app.state::<crate::commands::AppState>().registry.lock().list()
}

/// The "To:" candidates from the live panes. Labels come from the agent registry, never from a name match.
pub fn target_facts(infos: &[PaneInfo]) -> Vec<TargetFacts> {
    infos
        .iter()
        .map(|p| {
            let agent_label =
                crate::tui::descriptor_for(&p.agent_type).map(|d| d.label).unwrap_or_else(|| p.agent_type.clone());
            let title = p
                .title
                .as_deref()
                .map(|t| crate::redact::text(t.trim()).into_inner())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| agent_label.clone());
            TargetFacts {
                pane_id: p.id.clone(),
                agent_label,
                title,
                cwd: p.cwd.clone(),
                kind: PaneKind::of(crate::tui::profile_for(&p.agent_type).features().kind),
                run_controls: crate::bridge::pane_has_open_run(&p.id, &p.cwd),
                last_output_at: p.last_output_at,
            }
        })
        .collect()
}

/// The state the Point box shows now: picks, the "To:" list and the notice.
pub fn current_state(app: &AppHandle) -> PointState {
    let (opener, cwd) = newest_opener();
    let (targets, default) = target::targets(&target_facts(&pane_infos(app)), opener.as_deref(), cwd.as_deref());
    current_state_with(targets, default)
}

fn current_state_with(targets: Vec<target::Target>, default: Option<String>) -> PointState {
    SESSION.lock().state(now_ms(), targets, default)
}

/// The dev server pane of the newest pick's preview, else of the newest preview.
fn newest_opener() -> (Option<String>, Option<String>) {
    let last_preview = SESSION.lock().picks().last().map(|p| p.preview_id.clone());
    let previews = PREVIEWS.lock();
    let rec = last_preview
        .and_then(|id| previews.iter().find(|p| p.id == id))
        .or_else(|| previews.iter().max_by_key(|p| p.n));
    (rec.map(|p| p.opener.clone()), rec.map(|p| p.cwd.clone()))
}

/// Removes one pick (its shot file goes too) and returns the new state.
pub fn remove_pick(app: &AppHandle, pick_id: &str) -> PointState {
    let removed = SESSION.lock().remove(pick_id, now_ms());
    if let Some(shot) = removed.and_then(|p| p.shot) {
        delete_shots(&[shot.abs_path]);
    }
    current_state(app)
}

/// Throws the session away: deletes its shots, hides the box and turns the pickers off.
pub fn discard_all(app: &AppHandle) {
    let shots = SESSION.lock().discard();
    delete_shots(&shots);
    box_window::hide(app);
    let previews: Vec<Preview> = PREVIEWS.lock().clone();
    for rec in previews {
        rec.armed_until.store(0, Ordering::Relaxed);
        if let Some(window) = app.get_webview_window(&rec.id) {
            let _ = window.eval(call_script(&rec.key, "disarm", ""));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> PreviewOrigin {
        PreviewOrigin { scheme: "http".into(), host: "localhost".into(), port: 5173 }
    }

    #[test]
    fn labels_and_titles() {
        assert_eq!(preview_label(3), "preview-3");
        assert!(preview_label(1).starts_with(PREVIEW_PREFIX));
        assert_eq!(preview_title(&origin()), "Preview \u{b7} localhost:5173");
    }

    #[test]
    fn keys_are_random_hex_names() {
        let (a, b) = (new_key(), new_key());
        assert_ne!(a, b);
        assert_eq!(a.len(), 5 + 16);
        assert!(a.starts_with("__pm_") && a[5..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_picker_gets_its_origin_and_key_as_json_strings() {
        let script = picker_script(&origin(), "__pm_0123456789abcdef");
        assert!(!script.contains("__PM_ORIGIN__") && !script.contains("__PM_KEY__"));
        assert!(script.contains("const ORIGIN = \"http://localhost:5173\";"));
        assert!(script.contains("const KEY = \"__pm_0123456789abcdef\";"));
    }

    #[test]
    fn a_key_with_a_quote_cannot_break_out_of_the_script() {
        let script = picker_script(&origin(), "a\";alert(1);\"");
        assert!(script.contains(r#"const KEY = "a\";alert(1);\"";"#));
    }

    #[test]
    fn calls_into_the_picker_are_guarded_and_escaped() {
        let js = call_script("__pm_ab", "done", &serde_json::to_string("Sent \"it\"").unwrap());
        assert!(js.contains("window[\"__pm_ab\"]"));
        assert!(js.contains("a.done(\"Sent \\\"it\\\"\")"));
        assert!(js.contains("catch(e){return null}"));
        assert!(take_script("__pm_ab").contains("a.take()"));
    }

    #[test]
    fn only_a_json_string_counts_as_a_pick() {
        assert_eq!(parse_take(r#""{\"kind\":\"pick\"}""#), Some(r#"{"kind":"pick"}"#.to_string()));
        assert_eq!(parse_take("null"), None);
        assert_eq!(parse_take("42"), None);
        assert_eq!(parse_take(r#"{"kind":"pick"}"#), None);
        assert_eq!(parse_take(r#""""#), None);
        assert_eq!(parse_take("not json"), None);
        assert_eq!(parse_take(&format!("\"{}\"", "a".repeat(MAX_TAKE_BYTES))), None);
    }

    #[test]
    fn a_preview_opens_beside_the_cockpit_on_its_monitor() {
        // Cockpit on the left 40% of a monitor left of the primary one.
        let monitor: Rect4 = (-1920, 0, 1920, 1080);
        let cockpit: Rect4 = (-1920, 0, 768, 1080);
        let (x, y, w, h) = preview_bounds(monitor, cockpit, 0);
        assert_eq!((w, h), (1152, 918));
        assert_eq!(x, -1920 + 768);
        assert_eq!(y, (1080 - 918) / 2);
        assert!(x >= monitor.0 && x + w as i32 <= monitor.0 + monitor.2 as i32);
    }

    #[test]
    fn a_preview_goes_left_of_a_cockpit_on_the_right_edge() {
        let monitor: Rect4 = (0, 0, 1920, 1080);
        let cockpit: Rect4 = (1200, 0, 720, 1080);
        let (x, _, w, _) = preview_bounds(monitor, cockpit, 0);
        assert_eq!(x, 1200 - w as i32);
    }

    #[test]
    fn a_full_screen_cockpit_gives_a_preview_flush_right() {
        let monitor: Rect4 = (0, 0, 1920, 1080);
        let (x, _, w, _) = preview_bounds(monitor, (0, 0, 1920, 1080), 0);
        assert_eq!(x + w as i32, 1920);
    }

    #[test]
    fn previews_cascade_and_stay_on_the_monitor() {
        let monitor: Rect4 = (0, 0, 1920, 1080);
        let cockpit: Rect4 = (0, 0, 700, 1080);
        let first = preview_bounds(monitor, cockpit, 0);
        let second = preview_bounds(monitor, cockpit, 1);
        assert_eq!(second.0 - first.0, crate::app_windows::CASCADE_STEP as i32);
        assert_eq!(preview_bounds(monitor, cockpit, crate::app_windows::CASCADE_SLOTS), first);
        for slot in 0..20 {
            let (x, y, w, h) = preview_bounds(monitor, cockpit, slot);
            assert!(x >= 0 && y >= 0 && x + w as i32 <= 1920 && y + h as i32 <= 1080, "slot {slot}");
        }
    }

    #[test]
    fn element_rect_becomes_a_screen_rect() {
        let rect = super::super::Rect { x: 100.0, y: 50.0, w: 180.0, h: 44.0 };
        assert_eq!(screen_rect((1000, 200), &rect, 1.5), (1150, 275, 270, 66));
        let bad = super::super::Rect { x: f64::NAN, y: 0.0, w: -5.0, h: f64::INFINITY };
        assert_eq!(screen_rect((10, 20), &bad, 2.0), (10, 20, 0, 0));
    }

    #[test]
    fn only_our_own_shot_files_are_ever_deleted() {
        let shots = Path::new("work").join("app").join(".puppet-master").join("shots");
        assert!(is_own_shot(&shots.join("pick-20261028-223104-1.png")));
        assert!(!is_own_shot(&shots.join("notes.png")));
        assert!(!is_own_shot(&shots.join("pick-1.txt")));
        assert!(!is_own_shot(&Path::new("work").join("pick-1.png")));
        assert!(!is_own_shot(&Path::new("work").join("shots").join("pick-1.png")));
        assert!(!is_own_shot(&Path::new("")));
    }
}
