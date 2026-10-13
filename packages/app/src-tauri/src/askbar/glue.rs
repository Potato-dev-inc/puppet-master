//! The Tauri side of the Ask bar: the hidden window, the global shortcut, show/hide and events.
//! Pure rules live in `super`; nothing here runs in unit tests (no GUI allowed there).

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

static SHOWN_AT: AtomicU64 = AtomicU64::new(0);
static REGISTERED: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Registers the chosen combination with the OS through `tauri-plugin-global-shortcut`.
struct GlobalRegistrar<'a>(&'a AppHandle);

impl ShortcutRegistrar for GlobalRegistrar<'_> {
    fn register(&mut self, shortcut: &str) -> Result<(), String> {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
        let handle = self.0.clone();
        self.0
            .global_shortcut()
            .on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let handle = handle.clone();
                    std::thread::spawn(move || toggle(&handle));
                }
            })
            .map_err(|e| e.to_string())
    }

    fn unregister(&mut self, shortcut: &str) {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        let _ = self.0.global_shortcut().unregister(shortcut);
    }
}

/// Startup: load the saved settings and, only if the bar is enabled, register the shortcut and
/// create the hidden window. Nothing is shown and nothing takes focus.
pub fn init(app: &AppHandle) {
    let settings = load_settings();
    apply(app, &settings);
}

/// Bring the shortcut and the window in line with `settings` (also after a Settings change).
pub fn apply(app: &AppHandle, settings: &Settings) {
    let result = {
        let mut current = REGISTERED.lock();
        sync_shortcut(&mut GlobalRegistrar(app), &mut current, settings)
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, "ask bar shortcut could not be registered");
    }
    if settings.enabled {
        let app = app.clone();
        std::thread::spawn(move || {
            let _ = ensure_window(&app);
        });
    } else {
        hide(app);
    }
}

/// The bar's window, created hidden on first use.
pub fn ensure_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        return Ok(window);
    }
    let spec = window_spec();
    let window = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.url.into()))
        .title("Ask")
        .inner_size(spec.width, spec.height)
        .decorations(spec.decorations)
        .transparent(spec.transparent)
        .always_on_top(spec.always_on_top)
        .resizable(spec.resizable)
        .skip_taskbar(spec.skip_taskbar)
        .shadow(spec.shadow)
        .visible(spec.visible)
        .focused(spec.focused)
        .build()
        .map_err(|e| e.to_string())?;
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            if should_hide_on_blur(now_ms(), SHOWN_AT.load(Ordering::Relaxed)) {
                let _ = handle.hide();
            }
        }
    });
    Ok(window)
}

pub fn bar_visible(app: &AppHandle) -> bool {
    app.get_webview_window(WINDOW_LABEL).and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.hide();
    }
}

/// Resize for an answer (true) or back to the input line (false). Anchored at the top edge.
pub fn resize(app: &AppHandle, expanded: bool) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.set_size(LogicalSize::new(window_spec().width, height_for(expanded)));
    }
}

/// Show and focus the bar (the user's shortcut or an Inbox card). `fresh` tells the page to clear
/// itself and re-read the clipboard.
pub fn show(app: &AppHandle, fresh: bool) {
    let Ok(window) = ensure_window(app) else { return };
    let spec = window_spec();
    if fresh {
        let _ = window.set_size(LogicalSize::new(spec.width, spec.height));
    }
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let (x, y) = spotlight_position(
            (monitor.position().x, monitor.position().y, monitor.size().width, monitor.size().height),
            (spec.width * scale) as u32,
        );
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    SHOWN_AT.store(now_ms(), Ordering::Relaxed);
    let _ = window.show();
    let _ = window.set_focus();
    if fresh {
        let _ = app.emit_to(WINDOW_LABEL, "askbar://shown", ());
    }
}

/// The shortcut: hide when showing, show when hidden.
pub fn toggle(app: &AppHandle) {
    if bar_visible(app) {
        hide(app);
    } else {
        show(app, true);
    }
}

/// Text on the clipboard right now, if any.
pub fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// A finished answer nobody saw: keep it as a notice and tell the cockpit.
pub fn publish_notice(app: &AppHandle, result: &AskResult) {
    let notice = push_notice(result);
    let _ = app.emit("askbar://notice", &notice);
}

pub fn remove_notice(app: &AppHandle, id: &str) -> Option<Notice> {
    let notice = take_notice(id)?;
    let _ = app.emit("askbar://notice-removed", id);
    Some(notice)
}

/// Inbox "Open": show the bar with that old answer.
pub fn open_notice(app: &AppHandle, id: &str) -> bool {
    let Some(notice) = remove_notice(app, id) else { return false };
    show(app, false);
    resize(app, true);
    let _ = app.emit_to(WINDOW_LABEL, "askbar://show-result", &notice.result);
    true
}
