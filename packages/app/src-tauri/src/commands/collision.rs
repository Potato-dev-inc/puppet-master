//! Tauri commands behind Collision Alert (SPEC-n33 section 3.8). Thin: each one calls
//! `crate::collision::live`, which owns the snapshot, the ignore list and `collision.json`.
//!
//! Desktop window only: no bridge route, no MCP tool, no SSE. Nothing here can pause, stop or steer
//! an agent. Errors are `CODE: message` strings.

use crate::collision::live;
use crate::collision::model::{AlertView, Settings};

const ERR_BAD_ID: &str = "COLLISION_BAD_ID";

fn fail(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

/// An alert id is `ca-` and exactly 12 lowercase hex digits (`^ca-[0-9a-f]{12}$`).
fn valid_id(id: &str) -> bool {
    match id.strip_prefix("ca-") {
        Some(hex) => hex.len() == 12 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        None => false,
    }
}

/// `collision_list`: the last snapshot, newest first, hidden alerts left out. Never waits for a look.
#[tauri::command]
pub fn collision_list() -> Vec<AlertView> {
    live::views()
}

/// `collision_ignore`: hides one alert until a new file clashes for that pair. A malformed id is
/// `COLLISION_BAD_ID`; an id that is not on screen is `COLLISION_NOT_FOUND`. Changes nothing else.
#[tauri::command]
pub fn collision_ignore(id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err(fail(ERR_BAD_ID, "That heads-up id is not valid."));
    }
    live::ignore(&id)
}

/// `collision_settings_get`: the saved on/off switch (on by default).
#[tauri::command]
pub fn collision_settings_get() -> Settings {
    live::settings()
}

/// `collision_settings_set`: saves `collision.json` and returns what was saved.
/// A failed save is `COLLISION_SAVE_FAILED`.
#[tauri::command]
pub fn collision_settings_set(settings: Settings) -> Result<Settings, String> {
    live::set_settings(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_must_be_ca_and_twelve_lowercase_hex() {
        assert!(valid_id("ca-0123456789ab"));
        assert!(valid_id("ca-ffffffffffff"));
        let long = format!("ca-{}", "a".repeat(13));
        let bad = [
            "",
            "ca-",
            "ca-0123456789a",
            "ca-0123456789abc",
            long.as_str(),
            "CA-0123456789ab",
            "ca-0123456789AB",
            "ca-0123456789xz",
            "ca-../../etc/passwd",
            " ca-0123456789ab",
            "ca-0123456789ab ",
            "xca-0123456789ab",
        ];
        for id in bad {
            assert!(!valid_id(id), "{id:?} should be refused");
        }
    }

    #[test]
    fn a_malformed_id_is_refused_before_any_lookup() {
        for id in ["", "ca-xyz", "CA-0123456789ab", "ca-0123456789ab\u{202e}"] {
            let error = collision_ignore(id.to_string()).expect_err(id);
            assert!(error.starts_with("COLLISION_BAD_ID: "), "{error}");
        }
    }

    #[test]
    fn an_unknown_id_is_not_found_and_changes_nothing() {
        let before = collision_list();
        let error = collision_ignore("ca-ffffffffffff".to_string()).expect_err("unknown");
        assert!(error.starts_with("COLLISION_NOT_FOUND: "), "{error}");
        assert_eq!(collision_list(), before);
    }

    #[test]
    fn list_returns_the_last_snapshot() {
        let listed = collision_list();
        assert_eq!(listed, live::views());
        assert!(listed.len() <= crate::collision::model::MAX_ALERTS);
    }

    #[test]
    fn settings_round_trip_through_the_test_store() {
        let _guard = crate::collision::settings::TEST_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = collision_settings_get();

        let off = collision_settings_set(Settings { enabled: false }).expect("saved off");
        assert_eq!(off, Settings { enabled: false });
        assert_eq!(collision_settings_get(), Settings { enabled: false });

        let on = collision_settings_set(Settings { enabled: true }).expect("saved on");
        assert_eq!(on, Settings { enabled: true });
        assert_eq!(collision_settings_get(), Settings { enabled: true });

        collision_settings_set(before).expect("restored");
        assert_eq!(collision_settings_get(), before);
    }
}
