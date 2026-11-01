//! Reply costs: recorded per message id, read back, and capped at the newest KEEP.

use std::path::PathBuf;

use super::reply_cost::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-reply-cost-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_recorded_cost_comes_back_by_message_id() {
    let root = temp("roundtrip");
    assert!(load(&root).is_empty());
    record(&root, "m-aaa", 20_000_000).unwrap();
    record(&root, "m-bbb", 500).unwrap();
    let costs = load(&root);
    assert_eq!(costs.get("m-aaa"), Some(&20_000_000));
    assert_eq!(costs.get("m-bbb"), Some(&500));
    assert_eq!(costs.get("m-ccc"), None);
}

#[test]
fn only_the_newest_costs_are_kept() {
    let root = temp("cap");
    for i in 0..(KEEP + 3) {
        record(&root, &format!("m-{i}"), 1).unwrap();
    }
    let costs = load(&root);
    assert_eq!(costs.len(), KEEP);
    assert!(!costs.contains_key("m-0"));
    assert!(costs.contains_key(&format!("m-{}", KEEP + 2)));
}
