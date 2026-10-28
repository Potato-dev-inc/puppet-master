//! Tests for `live_room::active` (SPEC-n32 LR-12): the crash-cleanup file and what the next start does with it.
//! The relay is one on this computer (`pm_relay::spawn_local`); an unreachable one is a closed local port.

use std::path::PathBuf;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use pm_relay_protocol::room::{read_hash, RoomRegistration};

use super::*;

const HOUR_MS: u64 = 60 * 60 * 1000;
const NOW: u64 = 1_800_000_000_000;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-lr12-active-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn local_relay() -> String {
    format!("http://{}", pm_relay::spawn_local(pm_relay::Config::default()).0)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

/// Opens a room on the relay the way a start does, and returns its client.
fn open_room(relay: &str, key: &SigningKey) -> RoomClient {
    let client = RoomClient::new(relay, key.clone()).expect("client");
    client.register(&RoomRegistration::signed(key, now() + HOUR_MS, 5, read_hash(&[5u8; 16]))).expect("register");
    client
}

#[test]
fn the_file_names_the_room_and_holds_the_room_key_only() {
    let dir = temp_dir();
    let key = key(3);
    write(&dir, &Active::new("https://relay.example.test", &key, NOW)).unwrap();
    let text = std::fs::read_to_string(path(&dir)).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut names: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    names.sort();
    assert_eq!(names, ["ends_at_ms", "relay", "room_id", "signing_key_b64"]);
    let read_back = read(&dir).expect("reads back");
    assert_eq!(read_back.room_id, room_id(key.verifying_key().as_bytes()));
    assert_eq!(read_back.key().expect("key").to_bytes(), key.to_bytes());
}

#[test]
fn a_room_that_ends_late_does_not_remove_the_file_of_a_newer_room() {
    let dir = temp_dir();
    let (old, new) = (Active::new("https://relay.example.test", &key(1), NOW), Active::new("https://relay.example.test", &key(2), NOW));
    write(&dir, &new).unwrap();
    remove_if(&dir, &old.room_id);
    assert!(path(&dir).exists(), "another room's file stays");
    remove_if(&dir, &new.room_id);
    assert!(!path(&dir).exists());
}

#[test]
fn nothing_to_clean_when_there_is_no_file() {
    let dir = temp_dir();
    assert_eq!(cleanup_with(&dir, NOW, 1, Duration::ZERO), Cleanup::Nothing);
}

#[test]
fn a_damaged_or_mismatched_file_is_removed_without_a_call() {
    let dir = temp_dir();
    std::fs::create_dir_all(path(&dir).parent().unwrap()).unwrap();
    std::fs::write(path(&dir), "not json").unwrap();
    assert_eq!(cleanup_with(&dir, NOW, 1, Duration::ZERO), Cleanup::Dropped);
    assert!(!path(&dir).exists());

    // A key that does not name the room.
    let mut wrong = Active::new("http://127.0.0.1:1", &key(1), NOW + HOUR_MS);
    wrong.room_id = Active::new("http://127.0.0.1:1", &key(2), NOW).room_id;
    write(&dir, &wrong).unwrap();
    assert_eq!(cleanup_with(&dir, NOW, 1, Duration::ZERO), Cleanup::Dropped);
    assert!(!path(&dir).exists());
}

#[test]
fn a_room_past_its_time_is_dropped_without_a_call() {
    let dir = temp_dir();
    // The address would fail if it were called; the file is removed all the same.
    write(&dir, &Active::new("http://127.0.0.1:1", &key(1), NOW - 1)).unwrap();
    assert_eq!(cleanup_with(&dir, NOW, 1, Duration::ZERO), Cleanup::Dropped);
    assert!(!path(&dir).exists());
}

#[test]
fn a_leftover_room_is_deleted_on_the_relay_and_the_file_removed() {
    let relay = local_relay();
    let dir = temp_dir();
    let key = key(4);
    let client = open_room(&relay, &key);
    assert!(client.status().is_ok(), "the room is open");
    write(&dir, &Active::new(&relay, &key, now() + HOUR_MS)).unwrap();

    assert_eq!(cleanup_with(&dir, now(), 1, Duration::ZERO), Cleanup::Ended);
    assert!(!path(&dir).exists());
    assert!(matches!(client.status(), Err(ClientError::Gone(_))), "the relay has ended the room");
}

#[test]
fn a_leftover_room_the_relay_forgot_is_cleaned_all_the_same() {
    let relay = local_relay();
    let dir = temp_dir();
    write(&dir, &Active::new(&relay, &key(5), now() + HOUR_MS)).unwrap();
    assert_eq!(cleanup_with(&dir, now(), 1, Duration::ZERO), Cleanup::Ended);
    assert!(!path(&dir).exists());
}

#[test]
fn an_unreachable_relay_keeps_the_file_for_the_next_start() {
    let dir = temp_dir();
    write(&dir, &Active::new("http://127.0.0.1:1", &key(6), now() + HOUR_MS)).unwrap();
    assert_eq!(cleanup_with(&dir, now(), 2, Duration::from_millis(1)), Cleanup::KeptForNextStart);
    assert!(path(&dir).exists(), "the room key stays, so the next start can still end the room");
}
