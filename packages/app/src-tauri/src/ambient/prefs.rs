//! The one persisted ambient preference: "Animated tray icon" (default on). Stored in
//! `<app data>/ambient.json` so no shared settings file changes.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static ANIMATED: AtomicBool = AtomicBool::new(true);

pub fn animated() -> bool {
    ANIMATED.load(Ordering::Relaxed)
}

pub fn animated_from_json(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.get("animated_tray_icon").and_then(Value::as_bool))
        .unwrap_or(true)
}

fn path() -> PathBuf {
    crate::app_paths::app_data_dir().join("ambient.json")
}

pub fn load() {
    load_from(&path());
}

fn load_from(path: &Path) {
    let on = std::fs::read_to_string(path).map(|t| animated_from_json(&t)).unwrap_or(true);
    ANIMATED.store(on, Ordering::Relaxed);
}

pub fn set_animated(on: bool) -> std::io::Result<()> {
    ANIMATED.store(on, Ordering::Relaxed);
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serde_json::to_vec_pretty(&json!({ "animated_tray_icon": on }))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_defaults_on_and_reads_the_flag() {
        assert!(animated_from_json(""));
        assert!(animated_from_json("{}"));
        assert!(animated_from_json("not json"));
        assert!(!animated_from_json(r#"{"animated_tray_icon": false}"#));
        assert!(animated_from_json(r#"{"animated_tray_icon": true}"#));
    }
}
