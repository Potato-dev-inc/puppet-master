//! Actionable push notifications to the phone PWA.
//!
//! Flow: `bridge::push_pane_status_sse` / `bridge::push_operation_sse` call the two `notify_*`
//! hooks below -> `triggers::Gate` decides (entered waiting_input/error, duplicate prompt,
//! 30 s per pane) -> `hub::PushHub::plan` builds one encrypted-ready payload per subscribed phone
//! that is not looking at the PWA -> `sender` encrypts (RFC 8291) and POSTs it to the browser push
//! service on a worker thread.
//!
//! Everything about which agent it is comes from the registry (label via `tui::descriptors`), never
//! from matching agent names.

pub mod hub;
pub mod routes;
pub mod sender;
pub mod store;
pub mod triggers;
pub mod vapid;

use crate::mobile_pairing;
use crate::pty::PaneRegistry;
use hub::{Candidate, DeviceDirectory, PushHub};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use triggers::{Kind, PromptInfo, Verdict};

pub const VAPID_FILE_NAME: &str = "puppet-master.push-vapid.json";
pub const SUBSCRIPTIONS_FILE_NAME: &str = "puppet-master.push.json";

struct Ctx {
    hub: Arc<PushHub>,
    registry: Arc<Mutex<PaneRegistry>>,
}

static CTX: OnceCell<Ctx> = OnceCell::new();

/// The real directory: bearer tokens and paired devices live in the pairing store.
struct PairingDirectory;

impl DeviceDirectory for PairingDirectory {
    fn device_for_token(&self, token: &str) -> Option<String> {
        mobile_pairing::pairing_store()?.lock().device_id_for_token(token)
    }
    fn device_exists(&self, device_id: &str) -> bool {
        mobile_pairing::pairing_store()
            .is_some_and(|store| store.lock().list_devices().iter().any(|d| d.id == device_id))
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Create the hub (VAPID key + subscription file in the app data dir). Call once at startup.
pub fn init(registry: Arc<Mutex<PaneRegistry>>) {
    let dir = crate::app_paths::app_data_dir();
    let vapid = match vapid::VapidKeys::load_or_create(&dir.join(VAPID_FILE_NAME)) {
        Ok(vapid) => vapid,
        Err(err) => {
            tracing::warn!(%err, "push notifications unavailable: no VAPID key");
            return;
        }
    };
    let store = store::SubscriptionStore::load(dir.join(SUBSCRIPTIONS_FILE_NAME), vapid.public_key_b64url());
    let hub = Arc::new(PushHub::new(vapid, store, Box::new(PairingDirectory)));
    let _ = CTX.set(Ctx { hub, registry });
}

pub fn hub() -> Option<Arc<PushHub>> {
    CTX.get().map(|ctx| ctx.hub.clone())
}

/// Revoking a phone also drops its push subscription (called by the pairing store).
pub fn on_device_revoked(device_id: &str) {
    if let Some(hub) = hub() {
        hub.forget_device(device_id);
    }
}

/// The label an agent shows under in the UI, from its profile or definition.
fn agent_label(agent_type: &str) -> String {
    crate::tui::descriptors()
        .into_iter()
        .find(|d| d.id == agent_type)
        .map(|d| d.label)
        .unwrap_or_else(|| agent_type.to_string())
}

/// Hook for `push_pane_status_sse`. Cheap when nobody subscribed.
pub fn notify_pane_status(pane_id: &str, status: &str) {
    let Some(ctx) = CTX.get() else { return };
    let Some(kind) = ctx.hub.gate().lock().entered(pane_id, status) else { return };
    if !ctx.hub.has_subscribers() {
        return;
    }
    let pane_id = pane_id.to_string();
    let status = status.to_string();
    let _ = std::thread::Builder::new().name("push-pane".into()).spawn(move || {
        let Some(ctx) = CTX.get() else { return };
        // Let the dialog finish drawing and flickers settle, then make sure it is still true.
        std::thread::sleep(Duration::from_millis(triggers::SETTLE_MS));
        let (agent_type, current) = {
            let reg = ctx.registry.lock();
            match reg.panes.get(&pane_id) {
                Some(pane) => (pane.info.agent_type.clone(), pane.status.lock().as_str().to_string()),
                None => return,
            }
        };
        if current != status {
            return;
        }
        let prompt = (kind == Kind::WaitingInput)
            .then(|| crate::pane_prompt::detect(&ctx.registry, &pane_id, false))
            .flatten()
            .and_then(|value| PromptInfo::from_detected(&value));
        dispatch(
            ctx,
            Candidate {
                kind,
                key: pane_id.clone(),
                pane_id: Some(pane_id),
                agent_label: agent_label(&agent_type),
                prompt,
            },
        );
    });
}

/// Hook for `push_operation_sse`: a run reached a terminal state.
pub fn notify_operation(snapshot: &crate::operations::OperationSnapshot) {
    use crate::operations::OperationStatus;
    let Some(ctx) = CTX.get() else { return };
    let (kind, status) = match snapshot.status {
        OperationStatus::Completed => (Kind::RunFinished, "completed"),
        OperationStatus::Failed => (Kind::RunFailed, "failed"),
        _ => return,
    };
    if !ctx.hub.gate().lock().first_sight_of_operation(&snapshot.operation_id, status) {
        return;
    }
    if !ctx.hub.has_subscribers() {
        return;
    }
    let candidate = Candidate {
        kind,
        key: snapshot.pane_id.clone().unwrap_or_else(|| snapshot.operation_id.clone()),
        pane_id: snapshot.pane_id.clone(),
        agent_label: agent_label(&snapshot.agent_type),
        prompt: None,
    };
    let _ = std::thread::Builder::new().name("push-op".into()).spawn(move || {
        if let Some(ctx) = CTX.get() {
            dispatch(ctx, candidate);
        }
    });
}

fn dispatch(ctx: &Ctx, candidate: Candidate) {
    let (verdict, deliveries) = ctx.hub.plan(&candidate, now_ms());
    if verdict != Verdict::Send {
        tracing::debug!(?verdict, key = %candidate.key, "push notification not sent");
        return;
    }
    let subject = sender::vapid_subject();
    for delivery in deliveries {
        let request = match sender::build_request(
            &delivery.subscription,
            ctx.hub.vapid(),
            &subject,
            &delivery.payload,
            delivery.urgent,
        ) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!(%err, "push request could not be built");
                continue;
            }
        };
        match sender::send(&request) {
            sender::Outcome::Delivered => {}
            sender::Outcome::Gone => {
                let _ = ctx.hub.store().lock().remove_endpoint(&delivery.subscription.endpoint);
            }
            other => tracing::warn!(?other, "push service did not accept the notification"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoking_a_paired_device_drops_its_push_subscription() {
        // Real pairing store + the global hub: revoke_device must reach forget_device.
        let dir = std::env::temp_dir().join(format!("pm-push-revoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let vapid = vapid::VapidKeys::load_or_create(&dir.join("v.json")).unwrap();
        let store = store::SubscriptionStore::load(dir.join("s.json"), vapid.public_key_b64url());
        let hub = Arc::new(PushHub::new(vapid, store, Box::new(PairingDirectory)));
        let registry = Arc::new(Mutex::new(PaneRegistry::default()));
        assert!(CTX.set(Ctx { hub: hub.clone(), registry }).is_ok(), "only this test installs the global hub");

        let mut pairing = mobile_pairing::MobilePairingStore::load_or_create(dir.join("pairing.json")).unwrap();
        let session = pairing.create_pairing_session("https://pm.example.com/bridge".into());
        let paired = pairing
            .pair_device(mobile_pairing::PairRequestBody {
                pairing_code: session.pairing_code,
                device_name: "Test phone".into(),
                device_public_key: base64_key(),
            })
            .unwrap();
        assert_eq!(pairing.device_id_for_token(&paired.device_token).as_deref(), Some(paired.device_id.as_str()));

        let (sub, _, _) = store::test_subscription("https://fcm.googleapis.com/fcm/send/zzz");
        hub.store().lock().upsert(&paired.device_id, sub).unwrap();
        assert!(hub.has_subscribers());
        assert!(pairing.revoke_device(&paired.device_id).unwrap());
        assert!(!hub.has_subscribers(), "revoke must drop the subscription");
        assert_eq!(pairing.device_id_for_token(&paired.device_token), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn base64_key() -> String {
        use base64::Engine;
        let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        base64::engine::general_purpose::STANDARD.encode(key.verifying_key().as_bytes())
    }
}
