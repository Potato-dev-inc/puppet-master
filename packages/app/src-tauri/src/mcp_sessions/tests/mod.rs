//! Session ownership tests, split by topic. Shared fixtures live here.

use super::*;
use crate::tool_registry::McpMode;
use serde_json::Value;
use std::collections::HashSet;
use uuid::Uuid;

mod authorize;
mod identity;
mod ownership;
mod pane_hint;
mod pane_ids;
mod reconnect;

fn id() -> String {
    Uuid::new_v4().to_string()
}

fn panes() -> Vec<String> {
    vec![
        "57c03490-a9bd-4e4e-bde8-b65f54d25be3".into(),
        "57c0ffff-0000-4e4e-bde8-b65f54d25be3".into(),
        "9a1b2c3d-a9bd-4e4e-bde8-b65f54d25be3".into(),
    ]
}

#[test]
fn pane_ids_resolve_by_full_id_or_unique_prefix() {
    let ids = panes();
    assert_eq!(resolve_pane_id(&ids[0], &ids).unwrap(), ids[0]);
    assert_eq!(resolve_pane_id("57c03", &ids).unwrap(), ids[0]);
    assert_eq!(resolve_pane_id("9A1B", &ids).unwrap(), ids[2]);
}
