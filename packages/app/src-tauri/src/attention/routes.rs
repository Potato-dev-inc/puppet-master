//! The body of `GET /attention` (the bridge route is `bridge/routes/attention.rs`) and the Tauri
//! command `attention_snapshot`.

use super::caps::{self, DeviceCaps};
use serde_json::{json, Value};

/// The snapshot as JSON. A paired device also gets a `device` block with what it may do; the
/// desktop gets none, which means everything is allowed. The block is not in the broadcast frame
/// because that fanout is shared.
pub fn get(device: Option<DeviceCaps>) -> Value {
    let mut body = serde_json::to_value(super::snapshot()).unwrap_or_else(|_| json!({}));
    if let Some(device) = device {
        body["device"] = serde_json::to_value(device).unwrap_or(Value::Null);
    }
    body
}

/// For a paired device id (as the bridge route knows it).
pub fn get_for_device(device_id: &str) -> Value {
    get(Some(caps::for_device(device_id)))
}
