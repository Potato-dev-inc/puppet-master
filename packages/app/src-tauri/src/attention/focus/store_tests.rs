use super::model::*;
use super::store::*;
use std::fs;
use std::path::PathBuf;

const T0: u64 = 1_791_979_200_000;

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pm-focus-{name}-{}-{}", std::process::id(), uuid::Uuid::new_v4()))
}

fn running_state() -> State {
    let mut state = State::default();
    state.settings.default_minutes = 90;
    state.session = Some(Session::new(T0, 45, By::Desktop, new_id()));
    state.ledger.counts.add(Class::Companion);
    state
}

#[test]
fn a_missing_file_loads_as_focus_off_with_defaults() {
    let root = temp_root("missing");
    let store = Store::at(&root);
    let state = store.load();
    assert!(state.session.is_none());
    assert_eq!(state.settings, Settings::default());
    assert_eq!(state.v, FILE_VERSION);
    assert!(!root.exists(), "loading creates nothing");
}

#[test]
fn save_then_load_gives_back_the_same_state() {
    let root = temp_root("round");
    let store = Store::at(&root);
    let state = running_state();
    store.save(&state).unwrap();
    assert!(store.state_path().exists());
    let back = store.load();
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&state).unwrap());
    assert_eq!(back.session.as_ref().unwrap().ends_at_ms, T0 + 45 * MINUTE_MS);
    assert_eq!(back.ledger.counts.companion, 1);
}

#[test]
fn save_writes_no_temp_file_left_behind() {
    let root = temp_root("atomic");
    let store = Store::at(&root);
    store.save(&running_state()).unwrap();
    store.save(&State::default()).unwrap();
    assert!(!root.join("state.json.tmp").exists());
    assert!(store.load().session.is_none(), "the second save replaced the first");
}

#[test]
fn a_damaged_file_is_set_aside_and_read_as_empty() {
    let root = temp_root("damaged");
    let store = Store::at(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(store.state_path(), "{ this is not json").unwrap();

    let state = store.load();
    assert!(state.session.is_none(), "Focus starts off, pings flow");
    assert_eq!(state.settings, Settings::default());
    assert!(store.bad_path().exists(), "the damaged file is kept as state.json.bad");
    assert!(!store.state_path().exists(), "the damaged file is moved, not copied");
}

#[test]
fn a_future_version_file_is_set_aside() {
    let root = temp_root("future");
    let store = Store::at(&root);
    store.save(&running_state()).unwrap();
    let text = fs::read_to_string(store.state_path()).unwrap();
    let future = text.replacen("\"v\": 1", "\"v\": 2", 1);
    assert_ne!(text, future, "the test changed the version");
    fs::write(store.state_path(), future).unwrap();

    let state = store.load();
    assert!(state.session.is_none(), "a version this build does not know is never held");
    assert!(store.bad_path().exists());
}

#[test]
fn a_file_with_the_wrong_shape_is_set_aside() {
    let root = temp_root("shape");
    let store = Store::at(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(store.state_path(), r#"{"v":1,"settings":{"default_minutes":"forty"}}"#).unwrap();
    assert!(store.load().session.is_none());
    assert!(store.bad_path().exists());
}

#[test]
fn a_second_damaged_file_replaces_the_old_bad_copy() {
    let root = temp_root("twice");
    let store = Store::at(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(store.state_path(), "first damage").unwrap();
    store.load();
    fs::write(store.state_path(), "second damage").unwrap();
    store.load();
    assert_eq!(fs::read_to_string(store.bad_path()).unwrap(), "second damage");
}

#[test]
fn a_hand_edited_end_past_the_cap_is_pulled_back_on_load() {
    let root = temp_root("cap");
    let store = Store::at(&root);
    let mut state = running_state();
    if let Some(s) = state.session.as_mut() {
        s.ends_at_ms = T0 + 10_000 * MINUTE_MS;
    }
    store.save(&state).unwrap();
    let loaded = store.load();
    assert_eq!(loaded.session.unwrap().ends_at_ms, T0 + MAX_MINUTES as u64 * MINUTE_MS);
}

#[test]
fn new_ids_are_fc_and_twelve_lowercase_hex() {
    let id = new_id();
    assert!(id.starts_with("fc-"), "{id}");
    let hex = &id[3..];
    assert_eq!(hex.len(), 12, "{id}");
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{id}");
    assert_ne!(new_id(), new_id(), "ids differ");
}
