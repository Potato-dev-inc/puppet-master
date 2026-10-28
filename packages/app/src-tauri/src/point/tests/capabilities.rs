//! SPEC n32 S3: a preview page must have no way to call Puppet Master. Its window label `preview-<n>` may match
//! no capability window, and no capability may carry a `remote` key (that would open IPC to remote origins).
//! The Point box window `pointbox` is the one new window that does get capabilities. These tests read every
//! `capabilities/*.json` at test time, so a later edit cannot quietly break the rule.
use crate::point::window::PREVIEW_PREFIX;
use serde_json::Value;
use std::path::PathBuf;

/// Every capability file under the crate's `capabilities/` folder, parsed.
fn capability_files() -> Vec<(String, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("capabilities folder") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read capability");
        let json: Value = serde_json::from_str(&text).expect("capability is JSON");
        out.push((path.display().to_string(), json));
    }
    assert!(!out.is_empty(), "no capability files found");
    out
}

/// The window patterns of one capability. Tauri matches labels with `*` as "any run of characters".
fn window_patterns(json: &Value) -> Vec<String> {
    json.get("windows")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// True when `pattern` (with `*` wildcards) matches the whole `label`.
fn glob_matches(pattern: &str, label: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == label;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if label.len() < first.len() + last.len() || !label.starts_with(first) || !label.ends_with(last) {
        return false;
    }
    let mut rest = &label[first.len()..label.len() - last.len()];
    for middle in &parts[1..parts.len() - 1] {
        match rest.find(middle) {
            Some(at) => rest = &rest[at + middle.len()..],
            None => return false,
        }
    }
    true
}

/// True when a key named `remote` appears anywhere in the JSON.
fn has_remote_key(json: &Value) -> bool {
    match json {
        Value::Object(map) => map.contains_key("remote") || map.values().any(has_remote_key),
        Value::Array(items) => items.iter().any(has_remote_key),
        _ => false,
    }
}

#[test]
fn no_capability_window_matches_a_preview_label() {
    for (file, json) in capability_files() {
        for pattern in window_patterns(&json) {
            for label in [format!("{PREVIEW_PREFIX}1"), format!("{PREVIEW_PREFIX}x"), format!("{PREVIEW_PREFIX}12")] {
                assert!(!glob_matches(&pattern, &label), "{file}: window pattern `{pattern}` matches `{label}`");
            }
        }
    }
}

#[test]
fn no_capability_is_for_remote_origins() {
    for (file, json) in capability_files() {
        assert!(!has_remote_key(&json), "{file} has a `remote` key");
    }
}

#[test]
fn the_point_box_window_gets_the_default_capability() {
    let default = capability_files()
        .into_iter()
        .find(|(file, _)| file.ends_with("default.json"))
        .expect("capabilities/default.json");
    let patterns = window_patterns(&default.1);
    assert!(patterns.iter().any(|p| glob_matches(p, "pointbox")), "pointbox is not in {patterns:?}");
    assert!(patterns.iter().any(|p| glob_matches(p, "askbar")), "the existing askbar window went missing");
}

#[test]
fn the_glob_helper_matches_tauri_labels() {
    assert!(glob_matches("detached-pane-*", "detached-pane-3"));
    assert!(!glob_matches("detached-pane-*", "detached"));
    assert!(glob_matches("pointbox", "pointbox"));
    assert!(!glob_matches("pointbox", "pointbox2"));
    assert!(glob_matches("*", "preview-1"));
    assert!(!glob_matches("pre*-x", "preview-1"));
}
