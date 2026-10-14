//! A small hook for device and local-trust events. Today it only logs; the audit log (P0-9, owned
//! elsewhere) subscribes with [`subscribe`] and records what it needs. Emitting never blocks on
//! the subscriber's work beyond the call itself and never fails the request that caused it.

use parking_lot::RwLock;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecurityEvent {
    DevicePaired { device_id: String, caps: Vec<super::Cap> },
    DeviceRevoked { device_id: String },
    DeviceCapsChanged { device_id: String, caps: Vec<super::Cap> },
    /// A request carried the token of a device that went unused for the whole expiry window.
    DeviceExpired { device_id: String },
    /// A paired device asked for a route its caps do not cover (answered 403).
    CapDenied { device_id: String, method: String, route: String, needs: String },
    /// A loopback caller trusted as the app sent no (or a wrong) local secret. Log-only for now.
    LocalSecretMissing { method: String, route: String, wrong: bool },
}

type Listener = Box<dyn Fn(&SecurityEvent) + Send + Sync>;

static LISTENERS: once_cell::sync::Lazy<RwLock<Vec<Listener>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(Vec::new()));

/// Register a listener for every future event.
#[allow(dead_code)] // the audit log (P0-9) hooks in here
pub fn subscribe(listener: impl Fn(&SecurityEvent) + Send + Sync + 'static) {
    LISTENERS.write().push(Box::new(listener));
}

pub fn emit(event: SecurityEvent) {
    match &event {
        SecurityEvent::LocalSecretMissing { .. } => {} // rate-limited by local_secret.rs
        SecurityEvent::CapDenied { device_id, method, route, needs } => {
            tracing::info!(%device_id, %method, %route, %needs, "paired device refused: missing capability")
        }
        other => tracing::info!(event = ?other, "device security event"),
    }
    for listener in LISTENERS.read().iter() {
        listener(&event);
    }
}
