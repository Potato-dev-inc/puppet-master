//! Partial settings writes: `settings_patch(patch)` merges a JSON merge patch (RFC 7386) into the
//! `settings` object of `settings.json` on the Rust side, under one process-wide lock, and tells
//! every window with `settings://changed`.
//!
//! Why: the Settings window used to read the whole object, edit it and write the whole object
//! back, so a window holding an older copy wiped newer values (and `ui_mode`, which Rust owns).
//! A patch carries only the keys one control changed, so two windows can never clobber each other.
//!
//! The pure parts (`merge_patch`, `sanitize_patch`, `patch_file`, `write_atomic`) are unit tested
//! without a running app. The runtime part goes through the store plugin, because the frontend's
//! `LazyStore` reads that same in-memory store; editing the file behind its back would leave the
//! plugin with a stale copy that its next autosave writes over ours.

use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter};

pub const SETTINGS_FILE: &str = "settings.json";
const SETTINGS_KEY: &str = "settings";
pub const CHANGED_EVENT: &str = "settings://changed";

/// Keys a generic patch never writes. `ui_mode` has its own command (`set_ui_mode`) that also
/// moves windows around, so a stale `ui_mode` in a patch must be ignored, not applied.
pub const OWNED_ELSEWHERE: &[&str] = &["ui_mode"];

/// Provider keys. They live in the OS credential store when there is one; then they never touch the file.
pub const SECRET_KEYS: &[&str] = &["anthropic_api_key", "openai_api_key", "openrouter_api_key"];

/// One patch at a time: read-merge-write is not safe to interleave.
static PATCH_LOCK: Mutex<()> = Mutex::new(());
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// RFC 7386 JSON merge patch. Objects merge key by key, `null` removes a key, anything else
/// (strings, numbers, arrays) replaces the old value.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    let Some(patch_obj) = patch.as_object() else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let target_obj = target.as_object_mut().expect("just made an object");
    for (key, value) in patch_obj {
        if value.is_null() {
            target_obj.remove(key);
        } else {
            merge_patch(target_obj.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

/// The part of `patch` that may be written. Must be a JSON object. Drops keys owned elsewhere and,
/// when a credential store holds the provider keys, those too.
pub fn sanitize_patch(patch: &Value, drop_secrets: bool) -> Result<Map<String, Value>, String> {
    let obj = patch.as_object().ok_or("a settings patch must be a JSON object")?;
    Ok(obj
        .iter()
        .filter(|(key, _)| !OWNED_ELSEWHERE.contains(&key.as_str()))
        .filter(|(key, _)| !(drop_secrets && SECRET_KEYS.contains(&key.as_str())))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}

/// The patch as it may travel in an event: secret-looking keys are removed.
pub fn event_payload(patch: &Map<String, Value>) -> Value {
    Value::Object(patch.iter().filter(|(key, _)| !SECRET_KEYS.contains(&key.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect())
}

/// The whole file value with `patch` merged into its `settings` object. Other top-level keys stay.
pub fn merge_into_file(file: &Value, patch: &Map<String, Value>) -> Value {
    let mut out = if file.is_object() { file.clone() } else { json!({}) };
    if !out.get(SETTINGS_KEY).is_some_and(Value::is_object) {
        out[SETTINGS_KEY] = json!({});
    }
    merge_patch(&mut out[SETTINGS_KEY], &Value::Object(patch.clone()));
    out
}

/// Write `bytes` to `path` so a reader sees the old file or the new one, never half of one:
/// write a sibling temp file, then rename it over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = tmp_path(path);
    if let Err(e) = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| SETTINGS_FILE.into());
    path.with_file_name(format!("{name}.{}-{}.tmp", std::process::id(), TMP_COUNTER.fetch_add(1, Ordering::Relaxed)))
}

fn read_file_value(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or_else(|| json!({}))
}

/// Read-merge-write the file at `path`, atomically, and return the merged `settings` object.
/// Takes the patch lock, so concurrent callers queue. (The app goes through `apply`; this is the same
/// merge without the app, used by the tests.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn patch_file(path: &Path, patch: &Map<String, Value>) -> Result<Value, String> {
    let _guard = PATCH_LOCK.lock();
    patch_file_locked(path, patch)
}

pub(crate) fn patch_file_locked(path: &Path, patch: &Map<String, Value>) -> Result<Value, String> {
    let next = merge_into_file(&read_file_value(path), patch);
    let bytes = serde_json::to_vec_pretty(&next).map_err(|e| format!("could not encode settings: {e}"))?;
    write_atomic(path, &bytes).map_err(|e| format!("could not save settings: {e}"))?;
    Ok(next[SETTINGS_KEY].clone())
}

fn settings_path() -> PathBuf {
    crate::app_paths::app_data_dir().join(SETTINGS_FILE)
}

/// Merge through the store plugin (so the frontend's store sees it) and persist atomically.
fn patch_via_store(app: &AppHandle, patch: &Map<String, Value>) -> Result<Value, String> {
    use tauri::Manager;
    use tauri_plugin_store::StoreExt;
    let store = app.store(SETTINGS_FILE).map_err(|e| e.to_string())?;
    // The very path the store plugin resolves for its own saves (BaseDirectory::AppData).
    let path = app.path().app_data_dir().map_err(|e| e.to_string())?.join(SETTINGS_FILE);
    let current = store.get(SETTINGS_KEY).unwrap_or_else(|| json!({}));
    let mut next = if current.is_object() { current } else { json!({}) };
    merge_patch(&mut next, &Value::Object(patch.clone()));
    store.set(SETTINGS_KEY, next.clone());
    let mut file = Map::new();
    for (key, value) in store.entries() {
        file.insert(key, value);
    }
    let bytes = serde_json::to_vec_pretty(&Value::Object(file)).map_err(|e| e.to_string())?;
    write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(next)
}

/// The whole operation, with the side effects passed in so a test can run it without an app:
/// sanitize, merge under the lock via `persist`, then `mirror` the public part and `emit`
/// `settings://changed`. Returns the merged settings object.
pub fn apply_core(
    patch: &Value,
    drop_secrets: bool,
    persist: impl FnOnce(&Map<String, Value>) -> Result<Value, String>,
    mirror: impl FnOnce(&Value),
    emit: impl FnOnce(&str, Value),
) -> Result<Value, String> {
    let clean = sanitize_patch(patch, drop_secrets)?;
    let merged = {
        let _guard = PATCH_LOCK.lock();
        persist(&clean)?
    };
    if !clean.is_empty() {
        mirror(&Value::Object(clean.clone()));
        emit(CHANGED_EVENT, event_payload(&clean));
    }
    Ok(merged)
}

/// Apply `patch` for the running app: through the store plugin, falling back to the file.
pub fn apply(app: &AppHandle, patch: &Value) -> Result<Value, String> {
    apply_core(
        patch,
        crate::secrets::backend().available(),
        |clean| {
            if clean.is_empty() {
                return Ok(current_settings(app));
            }
            patch_via_store(app, clean).or_else(|store_err| {
                tracing::warn!(error = %store_err, "settings_patch: store plugin unusable, patching the file directly");
                patch_file_locked(&settings_path(), clean)
            })
        },
        |public| {
            crate::settings_store::mirror_public_patch(app, public);
        },
        |event, payload| {
            let _ = app.emit(event, payload);
        },
    )
}

fn current_settings(app: &AppHandle) -> Value {
    use tauri_plugin_store::StoreExt;
    app.store(SETTINGS_FILE)
        .ok()
        .and_then(|store| store.get(SETTINGS_KEY))
        .unwrap_or_else(|| read_file_value(&settings_path())[SETTINGS_KEY].clone())
}

/// Merge `patch` (only the keys that changed) into the saved settings. `null` removes a key.
#[tauri::command]
pub fn settings_patch(app: AppHandle, patch: Value) -> Result<Value, String> {
    apply(&app, &patch)
}

#[cfg(test)]
mod tests;
