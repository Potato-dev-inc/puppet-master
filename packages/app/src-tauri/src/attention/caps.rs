//! What the caller of an attention route may do, read from the device capabilities of
//! `mobile_pairing` (`Cap::Answer`, `Cap::Type`, `Cap::Control`). The desktop's own tooling is not a
//! device and is never limited.

use crate::mobile_pairing::{self, Cap, Caps};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DeviceCaps {
    pub can_answer: bool,
    pub can_type: bool,
    pub can_control: bool,
}

impl DeviceCaps {
    pub const fn all() -> Self {
        Self { can_answer: true, can_type: true, can_control: true }
    }

    pub const fn none() -> Self {
        Self { can_answer: false, can_type: false, can_control: false }
    }

    pub fn from_caps(caps: Caps) -> Self {
        Self { can_answer: caps.has(Cap::Answer), can_type: caps.has(Cap::Type), can_control: caps.has(Cap::Control) }
    }
}

/// The capabilities of a paired device. A device the store does not know has none (fail closed).
pub fn for_device(device_id: &str) -> DeviceCaps {
    mobile_pairing::pairing_store()
        .and_then(|store| store.lock().device_caps(device_id))
        .map(DeviceCaps::from_caps)
        .unwrap_or_else(DeviceCaps::none)
}

/// The name the person gave a paired device ("Anna's iPhone"), for the receipt other phones see.
pub fn device_name(device_id: &str) -> Option<String> {
    mobile_pairing::pairing_store()?.lock().list_devices().into_iter().find(|d| d.id == device_id).map(|d| d.name)
}
