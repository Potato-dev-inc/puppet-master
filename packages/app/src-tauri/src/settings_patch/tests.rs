use super::*;
use std::cell::RefCell;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-settings-patch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn merge_patch_follows_rfc_7386() {
    let mut target = json!({"a": 1, "b": {"c": 2, "d": 3}, "list": [1, 2], "gone": "x"});
    merge_patch(&mut target, &json!({"b": {"c": 9, "e": 4}, "list": [3], "gone": null, "new": true}));
    assert_eq!(target, json!({"a": 1, "b": {"c": 9, "d": 3, "e": 4}, "list": [3], "new": true}));
    // A patch object over a scalar starts a fresh object.
    let mut scalar = json!(5);
    merge_patch(&mut scalar, &json!({"x": 1}));
    assert_eq!(scalar, json!({"x": 1}));
}

#[test]
fn sanitize_drops_ui_mode_and_optionally_secrets_and_rejects_non_objects() {
    let patch = json!({"ui_mode": "individual", "theme": "light", "anthropic_api_key": "sk-1", "away_minutes": 7});
    let kept = sanitize_patch(&patch, false).unwrap();
    assert!(!kept.contains_key("ui_mode"), "ui_mode belongs to set_ui_mode");
    assert_eq!(kept["theme"], "light");
    assert_eq!(kept["anthropic_api_key"], "sk-1", "no credential store: the key may go to the file");
    let secure = sanitize_patch(&patch, true).unwrap();
    assert!(!secure.contains_key("anthropic_api_key"), "a credential store holds the key: never the file");
    assert_eq!(secure["away_minutes"], 7);
    assert!(sanitize_patch(&json!([1, 2]), false).is_err());
    assert!(sanitize_patch(&json!("theme"), false).is_err());
}

#[test]
fn patch_file_merges_keeps_other_keys_and_creates_missing_files() {
    let dir = temp_dir("merge");
    let path = dir.join("settings.json");
    // No file yet: it is created with just the patch.
    let merged = patch_file(&path, &obj(json!({"theme": "light"}))).unwrap();
    assert_eq!(merged, json!({"theme": "light"}));
    std::fs::write(&path, r#"{"settings":{"theme":"dark","ui_mode":"individual","away_minutes":5},"window":{"x":1}}"#).unwrap();
    let merged = patch_file(&path, &obj(json!({"away_minutes": 12, "checkpoints_keep": 80}))).unwrap();
    assert_eq!(merged["away_minutes"], 12);
    assert_eq!(merged["theme"], "dark", "keys outside the patch are untouched");
    assert_eq!(merged["ui_mode"], "individual");
    let file = read(&path);
    assert_eq!(file["settings"], merged);
    assert_eq!(file["window"], json!({"x": 1}), "other top-level keys survive");
    // null removes a key.
    patch_file(&path, &obj(json!({"checkpoints_keep": null}))).unwrap();
    assert!(read(&path)["settings"].get("checkpoints_keep").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreadable_file_is_replaced_not_crashed_on() {
    let dir = temp_dir("corrupt");
    let path = dir.join("settings.json");
    std::fs::write(&path, "{ not json").unwrap();
    let merged = patch_file(&path, &obj(json!({"theme": "light"}))).unwrap();
    assert_eq!(merged["theme"], "light");
    assert_eq!(read(&path)["settings"]["theme"], "light");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_atomic_replaces_the_file_and_leaves_no_temp_files() {
    let dir = temp_dir("atomic");
    let path = dir.join("settings.json");
    write_atomic(&path, b"{\"settings\":{\"a\":1}}").unwrap();
    write_atomic(&path, b"{\"settings\":{\"a\":2}}").unwrap();
    assert_eq!(read(&path)["settings"]["a"], 2);
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(leftovers, vec!["settings.json".to_string()], "the temp file was renamed away: {leftovers:?}");
    // A failed write (the target is a directory) cleans up its temp file and reports the error.
    let blocked = dir.join("blocked");
    std::fs::create_dir_all(blocked.join("child")).unwrap();
    assert!(write_atomic(&blocked, b"x").is_err());
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "no temp file left behind: {names:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The bug this module exists to fix: window A and window B both loaded the same snapshot. Each
/// then saves one control. With whole-object writes the second save puts the first one's old value
/// back; with patches both survive.
#[test]
fn two_stale_windows_do_not_clobber_each_other() {
    let dir = temp_dir("stale");
    let path = dir.join("settings.json");
    let initial = r#"{"settings":{"theme":"dark","away_minutes":5,"ui_mode":"orchestration"}}"#;
    std::fs::write(&path, initial).unwrap();
    let snapshot = read(&path)["settings"].clone(); // both windows hold this

    // The old model: each window writes back its whole (stale) copy with its own edit.
    let write_snapshot = |key: &str, value: Value| {
        let mut mine = snapshot.clone();
        mine[key] = value;
        let bytes = serde_json::to_vec_pretty(&json!({"settings": mine})).unwrap();
        write_atomic(&path, &bytes).unwrap();
    };
    write_snapshot("theme", json!("light")); // window A
    write_snapshot("away_minutes", json!(15)); // window B, still holding the old theme
    let old_model = read(&path)["settings"].clone();
    assert_eq!(old_model["theme"], "dark", "control: whole-snapshot writes lose window A's change");

    // The new model: the same two edits as patches.
    std::fs::write(&path, initial).unwrap();
    patch_file(&path, &obj(json!({"theme": "light"}))).unwrap(); // window A
    patch_file(&path, &obj(json!({"away_minutes": 15}))).unwrap(); // window B
    let merged = read(&path)["settings"].clone();
    assert_eq!(merged["theme"], "light");
    assert_eq!(merged["away_minutes"], 15);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stale_ui_mode_in_a_patch_never_reaches_the_file() {
    let dir = temp_dir("uimode");
    let path = dir.join("settings.json");
    std::fs::write(&path, r#"{"settings":{"ui_mode":"individual"}}"#).unwrap();
    // A stale window still thinks the mode is "orchestration" and sends it along with its edit.
    let result = apply_core(
        &json!({"ui_mode": "orchestration", "theme": "light"}),
        false,
        |clean| patch_file_locked(&path, clean),
        |_| {},
        |_, _| {},
    )
    .unwrap();
    assert_eq!(result["ui_mode"], "individual");
    assert_eq!(read(&path)["settings"]["theme"], "light");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_patches_from_many_threads_all_land() {
    let dir = temp_dir("threads");
    let path = dir.join("settings.json");
    std::fs::write(&path, r#"{"settings":{}}"#).unwrap();
    let handles: Vec<_> = (0..16)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || patch_file(&path, &obj(json!({ format!("key_{n}"): n }))).unwrap())
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let settings = read(&path)["settings"].clone();
    for n in 0..16 {
        assert_eq!(settings[format!("key_{n}")], n, "patch {n} was lost: {settings}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_core_emits_settings_changed_with_the_patch_but_never_a_key() {
    let dir = temp_dir("event");
    let path = dir.join("settings.json");
    let events: RefCell<Vec<(String, Value)>> = RefCell::new(Vec::new());
    let mirrored: RefCell<Vec<Value>> = RefCell::new(Vec::new());
    let merged = apply_core(
        &json!({"away_minutes": 9, "openai_api_key": "sk-secret", "ui_mode": "individual"}),
        false,
        |clean| patch_file_locked(&path, clean),
        |public| mirrored.borrow_mut().push(public.clone()),
        |event, payload| events.borrow_mut().push((event.to_string(), payload)),
    )
    .unwrap();
    assert_eq!(merged["away_minutes"], 9);
    assert_eq!(merged["openai_api_key"], "sk-secret", "no credential store: the file holds it");
    let events = events.into_inner();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "settings://changed");
    assert_eq!(events[0].1, json!({"away_minutes": 9}), "no ui_mode, no key in the event");
    assert_eq!(mirrored.into_inner().len(), 1, "the bridge mirror saw the patch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_or_invalid_patch_emits_nothing() {
    let dir = temp_dir("noevent");
    let path = dir.join("settings.json");
    std::fs::write(&path, r#"{"settings":{"theme":"dark"}}"#).unwrap();
    let count = RefCell::new(0);
    // Only an owned-elsewhere key: nothing to write, nothing to announce.
    let merged = apply_core(&json!({"ui_mode": "individual"}), false, |clean| patch_file_locked(&path, clean), |_| {}, |_, _| *count.borrow_mut() += 1).unwrap();
    assert_eq!(merged["theme"], "dark");
    assert_eq!(*count.borrow(), 0);
    assert!(apply_core(&json!(42), false, |c| patch_file_locked(&path, c), |_| {}, |_, _| *count.borrow_mut() += 1).is_err());
    assert_eq!(*count.borrow(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_write_reports_an_error_and_emits_nothing() {
    let dir = temp_dir("fail");
    let blocked = dir.join("settings.json");
    std::fs::create_dir_all(blocked.join("child")).unwrap(); // the target is a directory
    let count = RefCell::new(0);
    let result = apply_core(&json!({"theme": "light"}), false, |clean| patch_file_locked(&blocked, clean), |_| {}, |_, _| *count.borrow_mut() += 1);
    assert!(result.is_err());
    assert_eq!(*count.borrow(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
