//! Windows the Rust side opens by itself (panes-first PF2): the Settings window and pop-out pane
//! windows. Neither depends on the main webview. Window creation runs on a helper thread because
//! building a webview window from a non-main thread must not block an event-loop callback.

pub const SETTINGS_LABEL: &str = "settings";
pub const SETTINGS_TITLE: &str = "Puppet Master Settings";

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

mod runtime {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

    static STAGGER: AtomicUsize = AtomicUsize::new(0);

    /// Logical x/y just inside the main window (so a dev window lands on the monitor the app is
    /// on), staggered so windows do not stack exactly. `None` when there is no main window.
    fn near_main(app: &AppHandle) -> Option<(f64, f64)> {
        let main = app.get_webview_window("main")?;
        let pos = main.outer_position().ok()?;
        let scale = main.scale_factor().ok()?;
        let step = (STAGGER.fetch_add(1, Ordering::Relaxed) % 8) as f64 * 28.0;
        Some((f64::from(pos.x) / scale + 40.0 + step, f64::from(pos.y) / scale + 60.0 + step))
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
                return;
            }
            let mut builder = WebviewWindowBuilder::new(&app, SETTINGS_LABEL, WebviewUrl::App(settings_url(tab.as_deref()).into()))
                .title(SETTINGS_TITLE)
                .inner_size(1100.0, 760.0)
                .min_inner_size(720.0, 480.0)
                .resizable(true)
                .focused(focus);
            if let Some((x, y)) = near_main(&app) {
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

    /// Open (or show) the pop-out window of a pane. Does not take focus unless `focus`.
    pub fn open_pane_window(app: &AppHandle, pane_id: &str, agent: &str, cols: u16, rows: u16, focus: bool) {
        let label = crate::pane_windows::window_label(pane_id);
        if let Some(window) = app.get_webview_window(&label) {
            show(&window, true);
            return;
        }
        let (w, h) = pane_window_size(cols, rows);
        let title = format!("{agent} \u{b7} {}", pane_id.chars().take(8).collect::<String>());
        let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(pane_url(pane_id).into()))
            .title(title)
            .inner_size(w, h)
            .min_inner_size(260.0, 140.0)
            .resizable(true)
            .focused(focus);
        if let Some((x, y)) = near_main(app) {
            builder = builder.position(x, y);
        }
        match builder.build() {
            Ok(_) => tracing::info!(pane = %pane_id, "pane window opened"),
            Err(e) => tracing::warn!(pane = %pane_id, error = %e, "pane window failed to open"),
        }
    }

    /// Closing the Settings window only hides it: the app keeps running in the tray.
    pub fn on_close_requested(app: &AppHandle, label: &str) -> bool {
        if label != SETTINGS_LABEL {
            return false;
        }
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
        true
    }
}

pub use runtime::{on_close_requested, open_pane_window, open_settings_window};

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
    fn pane_url_matches_the_frontend_route() {
        assert_eq!(pane_url("abc-123"), "index.html?terminal=1&pane=abc-123&detached=1");
        assert_eq!(pane_url("a b/c"), "index.html?terminal=1&pane=a%20b%2Fc&detached=1");
    }

    #[test]
    fn pane_window_size_follows_the_grid_with_a_floor() {
        assert_eq!(pane_window_size(120, 30), (870.0, 480.0));
        assert_eq!(pane_window_size(0, 0), (870.0, 512.0));
        assert_eq!(pane_window_size(10, 2), (260.0, 140.0));
    }
}
