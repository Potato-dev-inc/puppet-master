//! Windows the Rust side opens by itself (panes-first PF2): the Settings window and pop-out pane
//! windows. Neither depends on the main webview. Window creation runs on a helper thread because
//! building a webview window from a non-main thread must not block an event-loop callback.

pub const SETTINGS_LABEL: &str = "settings";
pub const SETTINGS_TITLE: &str = "Puppet Master Settings";
pub const COCKPIT_LABEL: &str = crate::ui_mode::COCKPIT_LABEL;
pub const COCKPIT_TITLE: &str = "Puppet Master";

/// `index.html?cockpit=1`: the route `src/main.tsx` renders as the cockpit. A window created for a
/// "show this pane" request also carries `&select=<pane id>` so the selection survives the webview
/// not listening yet when `cockpit://select` is emitted.
pub fn cockpit_url(select: Option<&str>) -> String {
    match select.filter(|id| !id.is_empty()) {
        Some(id) => format!("index.html?cockpit=1&select={}", url_encode(id)),
        None => "index.html?cockpit=1".to_string(),
    }
}

/// `index.html?cockpit=1&view=settings&tab=bridge`: the cockpit window opened straight onto its inline Settings
/// view (Orchestration mode, window created by a "Settings" request). Not `settings=1`: that is the standalone
/// Settings window's route. The tab is sanitised like `settings_url`; a missing one lets the view choose.
pub fn cockpit_settings_url(tab: Option<&str>) -> String {
    let tab = tab.map(str::trim).filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    match tab {
        Some(tab) => format!("index.html?cockpit=1&view=settings&tab={tab}"),
        None => "index.html?cockpit=1&view=settings".to_string(),
    }
}

/// `index.html?settings=1&tab=agents`: the route `src/main.tsx` renders as the standalone Settings.
pub fn settings_url(tab: Option<&str>) -> String {
    let tab = tab.map(str::trim).filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    format!("index.html?settings=1&tab={}", tab.unwrap_or("agents"))
}

/// Same URL the frontend builds in `detachedPaneUrl` (`src/lib/detached-pane-window.ts`).
pub fn pane_url(pane_id: &str) -> String {
    let safe: String = url_encode(pane_id);
    format!("index.html?terminal=1&pane={safe}&detached=1")
}

/// URL of a pre-created (hidden) warm pane window: same route plus `warm=1`, which tells the window
/// its pane is not in any list yet (a warm worker is unlisted until a run or the tray claims it), so
/// it must not close itself as "pane gone".
pub fn warm_pane_url(pane_id: &str) -> String {
    format!("{}&warm=1", pane_url(pane_id))
}

/// What opening a pane's window has to do, given the window that may already exist (a Preload warm
/// window is created hidden and only needs placing and showing when its pane is claimed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPlan {
    /// No window yet: build one (placed and shown at creation).
    Create,
    /// A window exists but is hidden (warm): place it like a new window and show it.
    PlaceAndShow,
    /// A visible window exists: just bring it forward.
    Show,
}

pub fn open_plan(window_exists: bool, window_visible: bool) -> OpenPlan {
    match (window_exists, window_visible) {
        (false, _) => OpenPlan::Create,
        (true, false) => OpenPlan::PlaceAndShow,
        (true, true) => OpenPlan::Show,
    }
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Logical window size for a terminal of `cols` x `rows` (mirrors `detachedWindowSizeFromGrid`).
pub fn pane_window_size(cols: u16, rows: u16) -> (f64, f64) {
    let cols = if cols == 0 { 120 } else { cols };
    let rows = if rows == 0 { 32 } else { rows };
    (
        (f64::from(cols) * 7.25).ceil().max(260.0),
        (f64::from(rows) * 16.0).ceil().max(140.0),
    )
}

/// Top-left corner (logical pixels) new windows are placed relative to.
pub type Anchor = (f64, f64);

/// Horizontal/vertical offset between consecutive windows, and how many steps before wrapping.
pub const CASCADE_STEP: f64 = 28.0;
pub const CASCADE_SLOTS: usize = 8;
const CASCADE_ORIGIN: (f64, f64) = (40.0, 60.0);

/// Parse `"x,y"` (the dev anchor in `PM_WINDOW_ANCHOR`). Negative values are valid (a monitor to
/// the left of the primary one).
pub fn parse_anchor(text: &str) -> Option<Anchor> {
    let (x, y) = text.split_once(',')?;
    let (x, y) = (x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// `settings.pane_window_position` = `{ "x": n, "y": n }` from the app's `settings.json`
/// (`{"settings": {...}}`, written by the frontend store plugin).
pub fn anchor_from_settings(file: &serde_json::Value) -> Option<Anchor> {
    let pos = file.get("settings")?.get("pane_window_position")?;
    let (x, y) = (pos.get("x")?.as_f64()?, pos.get("y")?.as_f64()?);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// Where windows go: the dev anchor wins (it keeps dev windows on the secondary monitor), then
/// the user's setting, then the primary monitor's origin. `None` means "OS default".
pub fn resolve_anchor(dev: Option<Anchor>, setting: Option<Anchor>, primary: Option<Anchor>) -> Option<Anchor> {
    dev.or(setting).or(primary)
}

/// Position of the `slot`-th window opened from `anchor`: just inside the anchor, each next window
/// offset by `CASCADE_STEP`, wrapping after `CASCADE_SLOTS` so a long session never walks off screen.
pub fn cascade_position(anchor: Anchor, slot: usize) -> Anchor {
    let step = (slot % CASCADE_SLOTS) as f64 * CASCADE_STEP;
    (anchor.0 + CASCADE_ORIGIN.0 + step, anchor.1 + CASCADE_ORIGIN.1 + step)
}

mod runtime {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

    static STAGGER: AtomicUsize = AtomicUsize::new(0);

    /// Logical x/y for the next window (cascaded), or `None` to let the OS choose.
    fn next_position(app: &AppHandle) -> Option<(f64, f64)> {
        let dev = std::env::var("PM_WINDOW_ANCHOR").ok().and_then(|v| parse_anchor(&v));
        let setting = std::fs::read_to_string(crate::app_paths::app_data_dir().join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|json| anchor_from_settings(&json));
        let primary = app.primary_monitor().ok().flatten().map(|m| {
            let scale = m.scale_factor();
            (f64::from(m.position().x) / scale, f64::from(m.position().y) / scale)
        });
        let anchor = resolve_anchor(dev, setting, primary)?;
        Some(cascade_position(anchor, STAGGER.fetch_add(1, Ordering::Relaxed)))
    }

    fn show(window: &tauri::WebviewWindow, focus: bool) {
        let _ = window.unminimize();
        let _ = window.show();
        if focus {
            let _ = window.set_focus();
        }
    }

    /// Open the Settings window, or show the one that exists. `focus` is true when a person asked
    /// for it (tray click, second launch).
    pub fn open_settings_window(app: &AppHandle, tab: Option<&str>, focus: bool) {
        let app = app.clone();
        let tab = tab.map(str::to_string);
        std::thread::spawn(move || {
            if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
                show(&window, focus);
                // The window was hidden, not destroyed: tell it to reload its state and go to `tab`.
                let _ = tauri::Emitter::emit_to(&app, SETTINGS_LABEL, "settings://show", serde_json::json!({ "tab": tab }));
                return;
            }
            let mut builder = WebviewWindowBuilder::new(&app, SETTINGS_LABEL, WebviewUrl::App(settings_url(tab.as_deref()).into()))
                .title(SETTINGS_TITLE)
                .inner_size(1100.0, 760.0)
                .min_inner_size(720.0, 480.0)
                .resizable(true)
                .focused(focus);
            if let Some((x, y)) = next_position(&app) {
                builder = builder.position(x, y);
            }
            match builder.build() {
                Ok(window) => {
                    show(&window, focus);
                    tracing::info!("settings window opened");
                }
                Err(e) => tracing::warn!(error = %e, "settings window failed to open"),
            }
        });
    }

    /// Build the cockpit window at `url`. False when it could not be created.
    fn build_cockpit_window(app: &AppHandle, url: String, focus: bool) -> bool {
        let mut builder = WebviewWindowBuilder::new(app, COCKPIT_LABEL, WebviewUrl::App(url.into()))
            .title(COCKPIT_TITLE)
            .inner_size(1280.0, 820.0)
            .min_inner_size(900.0, 560.0)
            .resizable(true)
            .focused(focus);
        if let Some((x, y)) = next_position(app) {
            builder = builder.position(x, y);
        }
        match builder.build() {
            Ok(window) => {
                show(&window, focus);
                tracing::info!("cockpit window opened");
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, "cockpit window failed to open");
                false
            }
        }
    }

    /// Orchestration mode: show Settings INSIDE the cockpit (its inline view), on `tab` when given. The
    /// separate Settings window is hidden if it is up, so a person never sees two Settings surfaces. A new
    /// cockpit window carries the request in its URL; an existing one is told with `cockpit://settings`.
    pub fn open_cockpit_settings(app: &AppHandle, tab: Option<&str>) {
        use tauri::Emitter;
        let app = app.clone();
        let tab = tab.map(str::to_string);
        std::thread::spawn(move || {
            hide_settings_window(&app);
            let payload = serde_json::json!({ "tab": tab });
            if let Some(window) = app.get_webview_window(COCKPIT_LABEL) {
                show(&window, true);
                let _ = app.emit("cockpit://settings", payload);
                return;
            }
            if !build_cockpit_window(&app, cockpit_settings_url(tab.as_deref()), true) && app.get_webview_window(COCKPIT_LABEL).is_some() {
                // Another request created the window a moment ago; its page may not be listening yet.
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let _ = app.emit("cockpit://settings", payload);
            }
        });
    }

    /// Hide the Settings window if there is one (it is hidden, never destroyed, so it reloads fast next time).
    pub fn hide_settings_window(app: &AppHandle) {
        if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
            let _ = window.hide();
        }
    }

    /// The mode switched to Orchestration: if the Settings window is on screen, ask it to move into the cockpit
    /// (it knows which page it shows). Nothing happens for a hidden or missing window. Returns whether it asked.
    pub fn ask_settings_to_move_inline(app: &AppHandle) -> bool {
        match app.get_webview_window(SETTINGS_LABEL) {
            Some(window) if window.is_visible().unwrap_or(false) => {
                let _ = tauri::Emitter::emit_to(app, SETTINGS_LABEL, "settings://to-inline", serde_json::json!({}));
                true
            }
            _ => false,
        }
    }

    /// Open the cockpit window, or show the one that exists (unminimized). `select` also selects that
    /// pane (`cockpit://select`). `focus` is true when a person asked for it.
    pub fn open_cockpit_window(app: &AppHandle, select: Option<&str>, focus: bool) {
        use tauri::Emitter;
        let app = app.clone();
        let select = select.map(str::to_string);
        std::thread::spawn(move || {
            let mut created = false;
            if let Some(window) = app.get_webview_window(COCKPIT_LABEL) {
                show(&window, focus);
            } else if build_cockpit_window(&app, cockpit_url(select.as_deref()), focus) {
                created = true;
            } else {
                return;
            }
            if let Some(pane_id) = select {
                if created {
                    // The page is still loading; the URL carries the selection, this covers a
                    // listener that is already up.
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                }
                let _ = app.emit("cockpit://select", serde_json::json!({ "pane_id": pane_id }));
            }
        });
    }

    /// Hide the cockpit (individual mode); its panes keep running.
    pub fn hide_cockpit_window(app: &AppHandle) {
        if let Some(window) = app.get_webview_window(COCKPIT_LABEL) {
            let _ = window.hide();
        }
    }

    /// Destroy every pop-out pane window WITHOUT touching the panes. `destroy` skips the close
    /// request, so the "closing a pop-out kills its pane" hook never runs. Returns how many.
    pub fn destroy_pane_windows(app: &AppHandle) -> usize {
        let labels: Vec<String> =
            app.webview_windows().keys().filter(|l| l.starts_with("detached-pane-")).cloned().collect();
        let mut destroyed = 0;
        for label in labels {
            if let Some(window) = app.get_webview_window(&label) {
                if window.destroy().is_ok() {
                    destroyed += 1;
                }
            }
        }
        destroyed
    }

    /// Open (or show) the pop-out window of a pane. Does not take focus unless `focus`. A hidden
    /// warm window (Preload) is placed like a new one and shown: that is the whole cost of a claim.
    pub fn open_pane_window(app: &AppHandle, pane_id: &str, agent: &str, cols: u16, rows: u16, focus: bool) {
        let label = crate::pane_windows::window_label(pane_id);
        let existing = app.get_webview_window(&label);
        let visible = existing.as_ref().map(|w| w.is_visible().unwrap_or(true)).unwrap_or(false);
        match (open_plan(existing.is_some(), visible), existing) {
            (OpenPlan::Show, Some(window)) => show(&window, focus),
            (OpenPlan::PlaceAndShow, Some(window)) => {
                if let Some((x, y)) = next_position(app) {
                    let _ = window.set_position(tauri::LogicalPosition::new(x, y));
                }
                show(&window, focus);
                tracing::info!(pane = %pane_id, focus, "warm pane window shown");
            }
            _ => build_pane_window(app, &label, pane_id, agent, cols, rows, focus, false),
        }
    }

    /// Create the hidden window of a warm worker. Nothing is shown and nothing is placed yet.
    pub fn open_warm_pane_window(app: &AppHandle, pane_id: &str, agent: &str, cols: u16, rows: u16) {
        // Orchestration shows panes in the cockpit: a pop-out would only ever be destroyed on claim.
        if crate::ui_mode::current() == crate::ui_mode::UiMode::Orchestration {
            return;
        }
        let label = crate::pane_windows::window_label(pane_id);
        if app.get_webview_window(&label).is_none() {
            build_pane_window(app, &label, pane_id, agent, cols, rows, false, true);
        }
    }

    /// Destroy the window of a warm worker that is shut down unused (a window already shown belongs
    /// to a run and is left to the normal pane-removed sweep).
    pub fn close_warm_pane_window(app: &AppHandle, pane_id: &str) {
        let label = crate::pane_windows::window_label(pane_id);
        if let Some(window) = app.get_webview_window(&label) {
            if !window.is_visible().unwrap_or(true) {
                let _ = window.destroy();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_pane_window(app: &AppHandle, label: &str, pane_id: &str, agent: &str, cols: u16, rows: u16, focus: bool, warm: bool) {
        let (w, h) = pane_window_size(cols, rows);
        let title = crate::ambient::window_title(agent, pane_id, crate::ambient::store::get(pane_id).as_deref());
        let url = if warm { warm_pane_url(pane_id) } else { pane_url(pane_id) };
        let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
            .title(title)
            .inner_size(w, h)
            .min_inner_size(260.0, 140.0)
            .resizable(true)
            .focused(focus && !warm)
            .visible(!warm);
        if !warm {
            if let Some((x, y)) = next_position(app) {
                builder = builder.position(x, y);
            }
        }
        match builder.build() {
            Ok(_) => tracing::info!(pane = %pane_id, warm, "pane window opened"),
            Err(e) => tracing::warn!(pane = %pane_id, warm, error = %e, "pane window failed to open"),
        }
    }

    /// The standalone worker host (`--worker`) window, label `main` (the frontend routes it to the
    /// worker-host bootstrap). Only created in worker mode; the normal app's window is the cockpit or Settings.
    pub fn open_worker_host_window(app: &AppHandle) {
        if app.get_webview_window("main").is_some() {
            return;
        }
        let built = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("Puppet Master")
            .inner_size(1000.0, 700.0)
            .resizable(true)
            .build();
        if let Err(e) = built {
            tracing::warn!(error = %e, "worker host window failed to open");
        }
    }

    /// Closing the Settings window only hides it: the app keeps running in the tray.
    pub fn on_close_requested(app: &AppHandle, label: &str) -> bool {
        if label != SETTINGS_LABEL && label != COCKPIT_LABEL {
            return false;
        }
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
        true
    }
}

pub use runtime::{
    ask_settings_to_move_inline, close_warm_pane_window, destroy_pane_windows, hide_cockpit_window, hide_settings_window,
    on_close_requested, open_cockpit_settings, open_cockpit_window, open_pane_window, open_settings_window,
    open_warm_pane_window, open_worker_host_window,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_url_defaults_to_the_agents_tab_and_rejects_odd_tab_names() {
        assert_eq!(settings_url(None), "index.html?settings=1&tab=agents");
        assert_eq!(settings_url(Some("mcp")), "index.html?settings=1&tab=mcp");
        assert_eq!(settings_url(Some("a&b=c")), "index.html?settings=1&tab=agents");
        assert_eq!(settings_url(Some("")), "index.html?settings=1&tab=agents");
    }

    #[test]
    fn cockpit_settings_url_is_the_cockpit_route_not_the_settings_window_route() {
        assert_eq!(cockpit_settings_url(None), "index.html?cockpit=1&view=settings");
        assert_eq!(cockpit_settings_url(Some("bridge")), "index.html?cockpit=1&view=settings&tab=bridge");
        // `settings=1` would make main.tsx render the standalone Settings window instead of the cockpit.
        assert!(!cockpit_settings_url(Some("agents")).contains("settings=1"));
        // Anything that is not a plain page id is dropped, like `settings_url`.
        assert_eq!(cockpit_settings_url(Some("a&b=c")), "index.html?cockpit=1&view=settings");
        assert_eq!(cockpit_settings_url(Some("  ")), "index.html?cockpit=1&view=settings");
    }

    #[test]
    fn cockpit_url_matches_the_frontend_route() {
        assert_eq!(cockpit_url(None), "index.html?cockpit=1");
        assert_eq!(cockpit_url(Some("")), "index.html?cockpit=1");
        assert_eq!(cockpit_url(Some("a b")), "index.html?cockpit=1&select=a%20b");
        assert_eq!(COCKPIT_LABEL, "cockpit");
    }

    #[test]
    fn anchors_parse_from_dev_env_and_settings_file() {
        assert_eq!(parse_anchor("-1660,60"), Some((-1660.0, 60.0)));
        assert_eq!(parse_anchor(" 10 , 20.5 "), Some((10.0, 20.5)));
        assert_eq!(parse_anchor("10"), None);
        assert_eq!(parse_anchor("a,b"), None);
        let file = serde_json::json!({"settings": {"pane_window_position": {"x": -100, "y": 5}}});
        assert_eq!(anchor_from_settings(&file), Some((-100.0, 5.0)));
        assert_eq!(anchor_from_settings(&serde_json::json!({"settings": {}})), None);
        assert_eq!(anchor_from_settings(&serde_json::json!({"settings": {"pane_window_position": {"x": 1}}})), None);
    }

    #[test]
    fn anchor_priority_is_dev_then_setting_then_primary() {
        let (d, s, p) = (Some((1.0, 1.0)), Some((2.0, 2.0)), Some((3.0, 3.0)));
        assert_eq!(resolve_anchor(d, s, p), d);
        assert_eq!(resolve_anchor(None, s, p), s);
        assert_eq!(resolve_anchor(None, None, p), p);
        assert_eq!(resolve_anchor(None, None, None), None);
    }

    #[test]
    fn cascade_offsets_each_window_and_wraps() {
        let anchor = (-1660.0, 60.0);
        assert_eq!(cascade_position(anchor, 0), (-1620.0, 120.0));
        assert_eq!(cascade_position(anchor, 1), (-1592.0, 148.0));
        assert_eq!(cascade_position(anchor, CASCADE_SLOTS), cascade_position(anchor, 0));
        let all: std::collections::HashSet<_> =
            (0..CASCADE_SLOTS).map(|i| format!("{:?}", cascade_position(anchor, i))).collect();
        assert_eq!(all.len(), CASCADE_SLOTS);
    }

    #[test]
    fn pane_url_matches_the_frontend_route() {
        assert_eq!(pane_url("abc-123"), "index.html?terminal=1&pane=abc-123&detached=1");
        assert_eq!(pane_url("a b/c"), "index.html?terminal=1&pane=a%20b%2Fc&detached=1");
    }

    #[test]
    fn warm_windows_use_the_pane_route_with_a_warm_flag() {
        assert_eq!(warm_pane_url("abc-123"), "index.html?terminal=1&pane=abc-123&detached=1&warm=1");
    }

    #[test]
    fn claiming_a_warm_pane_places_and_shows_its_hidden_window_instead_of_building_one() {
        assert_eq!(open_plan(false, false), OpenPlan::Create);
        assert_eq!(open_plan(true, false), OpenPlan::PlaceAndShow, "hidden warm window: show only");
        assert_eq!(open_plan(true, true), OpenPlan::Show);
    }

    #[test]
    fn pane_window_size_follows_the_grid_with_a_floor() {
        assert_eq!(pane_window_size(120, 30), (870.0, 480.0));
        assert_eq!(pane_window_size(0, 0), (870.0, 512.0));
        assert_eq!(pane_window_size(10, 2), (260.0, 140.0));
    }
}
