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

pub mod companion_replies;
pub mod hub;
pub mod routes;
pub mod sender;
pub mod store;
pub mod triggers;
pub mod vapid;

#[cfg(test)]
mod tests_golden;

use crate::mobile_pairing;
use crate::pty::PaneRegistry;
use companion_replies::{Message, Release, Step};
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
            .is_some_and(|store| store.lock().is_active_device(device_id))
    }
    fn device_can_answer(&self, device_id: &str) -> bool {
        // Fails closed: a device the store does not know gets no Allow button.
        mobile_pairing::pairing_store().is_some_and(|store| {
            store
                .lock()
                .device_caps(device_id)
                .is_some_and(|caps| caps.has(mobile_pairing::Cap::Answer))
        })
    }
    fn device_can_watch(&self, device_id: &str) -> bool {
        // Fails closed: a device the store does not know gets no companion reply push.
        mobile_pairing::pairing_store().is_some_and(|store| {
            store
                .lock()
                .device_caps(device_id)
                .is_some_and(|caps| caps.has(mobile_pairing::Cap::Watch))
        })
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

/// What the sidebar calls a pane: the person's title (or the auto title), else the agent label
/// numbered when several panes run the same agent ("Claude Code 2"). `None` for a pane that is gone.
fn pane_heading(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<String> {
    use crate::attention::copy::{headings, PaneName};
    let names: Vec<PaneName> = registry
        .lock()
        .list()
        .into_iter()
        .map(|p| PaneName { id: p.id, agent_type: p.agent_type, created_at: p.created_at, title: p.title })
        .collect();
    headings(&names, agent_label).remove(pane_id)
}

/// Send what a "hidden" presence report released (see `PushHub::set_visible`), off-thread.
pub fn flush_outbox(hub: &Arc<PushHub>) {
    let deliveries = hub.take_outbox();
    if deliveries.is_empty() {
        return;
    }
    let hub = hub.clone();
    let _ = std::thread::Builder::new()
        .name("push-rearm".into())
        .spawn(move || send_deliveries(&hub, deliveries));
}

/// Hook for `push_pane_status_sse`. Cheap when nobody subscribed.
pub fn notify_pane_status(pane_id: &str, status: &str) {
    let Some(ctx) = CTX.get() else { return };
    let Some(kind) = ctx.hub.note_pane_status(pane_id, status) else { return };
    // A Night Shift pane is quiet: no push while the person sleeps (the morning push covers it).
    if crate::night_shift::quiet::is_quiet_pane(pane_id) {
        return;
    }
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
        let heading = pane_heading(&ctx.registry, &pane_id);
        if current != status {
            return;
        }
        let detected = (kind == Kind::WaitingInput).then(|| crate::pane_prompt::detect(&ctx.registry, &pane_id, false)).flatten();
        let prompt = detected.as_ref().and_then(PromptInfo::from_detected);
        // A folder-trust or sign-in screen only the person may pass is its own kind of notification.
        let gate = detected
            .as_ref()
            .filter(|value| {
                value.get("kind").and_then(serde_json::Value::as_str).is_some_and(|k| crate::pane_prompt::STARTUP_GATE_KINDS.contains(&k))
            })
            .map(|value| {
                let has_choices = value.get("choices").and_then(serde_json::Value::as_array).is_some_and(|c| !c.is_empty());
                has_choices.then(|| project_of(&ctx.registry, &pane_id).unwrap_or_else(|| "this folder".to_string()))
            });
        let (kind, detail) = match gate {
            Some(folder) => (Kind::TrustGate, folder),
            None => (kind, None),
        };
        let item = item_ref_for_pane(&pane_id, kind);
        dispatch(
            ctx,
            Candidate {
                kind,
                key: pane_id.clone(),
                pane_id: Some(pane_id),
                agent_label: agent_label(&agent_type),
                heading,
                prompt,
                item,
                detail,
            },
        );
    });
}

/// The last folder of a pane's working directory ("shop"), for "Trust this folder? · shop".
fn project_of(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<String> {
    let cwd = registry.lock().panes.get(pane_id).map(|pane| pane.info.cwd.clone())?;
    cwd.split(['\\', '/']).filter(|part| !part.is_empty()).last().map(str::to_string)
}

/// The attention card a pane's notification is about: its open approval, trust gate or error, with
/// the occurrence and fingerprint the card shows. Read from the live list (the notification has
/// waited out `SETTLE_MS`, so the list has the prompt).
fn item_ref_for_pane(pane_id: &str, kind: Kind) -> Option<hub::ItemRef> {
    use crate::attention::model::{Kind as Item, State};
    let wanted = match kind {
        Kind::TrustGate => Item::TrustGate,
        Kind::WaitingInput => Item::Approval,
        Kind::Error => Item::Error,
        _ => return None,
    };
    let snapshot = crate::attention::snapshot_now();
    let item = snapshot.items.iter().find(|i| i.state == State::Open && i.kind == wanted && i.pane.as_ref().is_some_and(|p| p.id == pane_id))?;
    Some(item_ref(item))
}

fn item_ref(item: &crate::attention::model::Item) -> hub::ItemRef {
    hub::ItemRef {
        id: item.id.clone(),
        occurrence: item.prompt.as_ref().map(|p| p.occurrence).unwrap_or(0),
        fingerprint: item.prompt.as_ref().map(|p| p.fingerprint.clone()).unwrap_or_default(),
    }
}

/// The attention card of a question, consent, budget or usage-limit request.
fn item_ref_for_question(question: &crate::questions::Question, kind: Kind) -> Option<hub::ItemRef> {
    use crate::attention::model::State;
    let prefix = match kind {
        Kind::Consent => "consent",
        Kind::Budget => "budget",
        Kind::Limit => "limit",
        _ => "question",
    };
    let key = format!("{prefix}:{}", question.id);
    let snapshot = crate::attention::snapshot_now();
    snapshot.items.iter().find(|i| i.state == State::Open && i.key == key).map(item_ref)
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
    // A Night Shift pane's run result is quiet: the night report carries it.
    if snapshot.pane_id.as_deref().is_some_and(crate::night_shift::quiet::is_quiet_pane) {
        return;
    }
    if !ctx.hub.gate().lock().first_sight_of_operation(&snapshot.operation_id, status) {
        return;
    }
    if !ctx.hub.has_subscribers() {
        return;
    }
    let heading = snapshot.pane_id.as_deref().and_then(|id| pane_heading(&ctx.registry, id));
    let candidate = Candidate {
        kind,
        key: snapshot.pane_id.clone().unwrap_or_else(|| snapshot.operation_id.clone()),
        pane_id: snapshot.pane_id.clone(),
        agent_label: agent_label(&snapshot.agent_type),
        heading,
        prompt: None,
        item: None,
        detail: None,
    };
    let _ = std::thread::Builder::new().name("push-op".into()).spawn(move || {
        if let Some(ctx) = CTX.get() {
            dispatch(ctx, candidate);
        }
    });
}

/// Hook for the companion keeper: a reply went into a companion's feed. `enabled` is the companion's
/// `push_replies`. The decision is made here (cheap); the sending runs on a thread of its own, so the
/// keeper never waits on the network.
pub fn notify_companion_reply(companion_id: &str, name: &str, text: &str, enabled: bool) {
    let Some(ctx) = CTX.get() else { return };
    if !enabled || !ctx.hub.has_subscribers() {
        return;
    }
    let step = ctx.hub.companion_replies().lock().reply(companion_id, name, text, now_ms());
    match step {
        Step::Skip => {}
        Step::Send(message) => {
            let _ = std::thread::Builder::new().name("push-reply".into()).spawn(move || {
                if let Some(ctx) = CTX.get() {
                    send_companion(ctx, &message);
                }
            });
        }
        Step::Held { arm: true } => {
            let owned = companion_id.to_string();
            let spawned = std::thread::Builder::new().name("push-reply-timer".into()).spawn(move || run_reply_timer(&owned));
            if spawned.is_err() {
                if let Some(ctx) = CTX.get() {
                    ctx.hub.companion_replies().lock().disarm(companion_id);
                }
            }
        }
        Step::Held { arm: false } => {}
    }
}

/// Waits out a companion's window and sends what it held (`Gate::release`). One per companion at a time.
fn run_reply_timer(companion_id: &str) {
    loop {
        let Some(ctx) = CTX.get() else { return };
        let now = now_ms();
        let release = ctx.hub.companion_replies().lock().release(companion_id, now);
        match release {
            Release::Send(message) => {
                send_companion(ctx, &message);
                return;
            }
            Release::Wait(due) => std::thread::sleep(Duration::from_millis(due.saturating_sub(now).clamp(50, 5_000))),
            Release::Done => return,
        }
    }
}

fn send_companion(ctx: &Ctx, message: &Message) {
    let deliveries = ctx.hub.companion_deliveries(message, now_ms());
    send_deliveries(&ctx.hub, deliveries);
}

/// Test spy: the ids `notify_question` was called with (the global hub is not installed in tests).
#[cfg(test)]
pub static NOTIFIED_QUESTIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Hook for `ask_user`: a question was just created. Fires once per question (the caller only
/// invokes it on creation). Tapping the notification opens the phone's question list.
pub fn notify_question(question: &crate::questions::Question) {
    // A question from a Night Shift pane waits for the morning report; it sends nothing now.
    if question.pane_id.as_deref().is_some_and(crate::night_shift::quiet::is_quiet_pane) {
        return;
    }
    #[cfg(test)]
    NOTIFIED_QUESTIONS.lock().push(question.id.clone());
    let Some(ctx) = CTX.get() else { return };
    if !ctx.hub.has_subscribers() {
        return;
    }
    // The registry's label is trusted; the agent-supplied one is only a fallback (and sanitised).
    let from_pane = question.pane_id.as_deref().and_then(|id| {
        let reg = ctx.registry.lock();
        reg.panes.get(id).map(|pane| pane.info.agent_type.clone())
    });
    let given = question.agent.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let label = match (from_pane, given) {
        (Some(agent_type), _) => agent_label(&agent_type),
        (None, Some(given)) => triggers::truncate_chars(given, 40),
        (None, None) => String::new(),
    };
    // Puppet Master's own cards (consent, a spent budget, a usage limit) each say what they are.
    let (kind, label, detail) = classify_question(question, label);
    let candidate = Candidate {
        kind,
        key: format!("q-{}", question.id),
        pane_id: None,
        agent_label: if label.is_empty() { "An agent".to_string() } else { label },
        heading: None,
        prompt: Some(PromptInfo { prompt_id: String::new(), text: question.question.clone(), actionable: false, risk: None }),
        item: None,
        detail,
    };
    let question = question.clone();
    let _ = std::thread::Builder::new().name("push-question".into()).spawn(move || {
        if let Some(ctx) = CTX.get() {
            let mut candidate = candidate;
            candidate.item = item_ref_for_question(&question, candidate.kind);
            dispatch(ctx, candidate);
        }
    });
}

/// What kind of notification a question is, who is speaking, and the fact its first line adds.
fn classify_question(question: &crate::questions::Question, label: String) -> (Kind, String, Option<String>) {
    use triggers::truncate_chars;
    if let Some(consent) = &question.consent {
        let requester = truncate_chars(&consent.requester, 40);
        let target = truncate_chars(&consent.target, 40);
        let sentence = match consent.action.as_str() {
            "answer_prompt" => format!("{requester} wants to answer in {target}"),
            _ => format!("{requester} wants to control {target}"),
        };
        return (Kind::Consent, requester, Some(sentence));
    }
    if question.kind.as_deref() == Some(crate::budgets::CARD_KIND) {
        // "20 of 20 turns. It's paused after its last turn..." -> "20 of 20 turns"
        let used = question.context.as_deref().and_then(|c| c.split(". ").next()).map(|s| s.trim_end_matches('.').to_string());
        return (Kind::Budget, label, used);
    }
    if let Some(limit) = &question.limit {
        let reset = limit.reset_text.clone().filter(|t| !t.trim().is_empty());
        return (Kind::Limit, truncate_chars(&limit.agent_label, 40), reset);
    }
    if matches!(question.kind.as_deref(), Some(k) if k == crate::questions::ISSUE_KIND_START || k == crate::questions::ISSUE_KIND_PUSH) {
        // An issue card (`issue_intake`) is worded as a question, from its title line only: the context (the
        // issue text) never reaches a lock screen.
        return (Kind::Question, "GitHub issue".to_string(), Some(question.question.chars().take(60).collect()));
    }
    (Kind::Question, label, None)
}

fn dispatch(ctx: &Ctx, candidate: Candidate) {
    let (verdict, deliveries) = ctx.hub.plan(&candidate, now_ms());
    if verdict == Verdict::RateLimited {
        // Held, not dropped: it goes out when the window ends, if it is still true (`release_held`).
        tracing::debug!(key = %candidate.key, "push notification held back by the rate limit");
        start_releaser();
        return;
    }
    if verdict != Verdict::Send {
        tracing::debug!(?verdict, key = %candidate.key, "push notification not sent");
        return;
    }
    send_deliveries(&ctx.hub, deliveries);
}

static RELEASER_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One thread at a time wakes when a held notification's window ends and releases it.
fn start_releaser() {
    use std::sync::atomic::Ordering;
    if RELEASER_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("push-held".into()).spawn(|| {
        loop {
            let Some(ctx) = CTX.get() else { break };
            let now = now_ms();
            for held in ctx.hub.take_due(now) {
                release_held(ctx, held, now);
            }
            match ctx.hub.next_held_due() {
                Some(at) => std::thread::sleep(Duration::from_millis(at.saturating_sub(now_ms()).clamp(50, 5_000))),
                None => break,
            }
        }
        RELEASER_RUNNING.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        RELEASER_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// How many blocking things are open for a held candidate right now, from the attention list.
fn open_things(held: &hub::Held) -> u32 {
    use crate::attention::model::{Lane, State};
    let snapshot = crate::attention::snapshot_now();
    let candidate = &held.candidate;
    let question_id = candidate.key.strip_prefix("q-");
    snapshot
        .items
        .iter()
        .filter(|item| item.state == State::Open && item.lane == Lane::Blocking)
        .filter(|item| match (&candidate.pane_id, question_id) {
            (Some(pane_id), _) => item.pane.as_ref().is_some_and(|p| &p.id == pane_id),
            (None, Some(id)) => item.key == format!("question:{id}") || item.key == format!("consent:{id}"),
            (None, None) => false,
        })
        .count() as u32
}

/// A held notification's window ended: say what is still true (nothing, the one thing, or one
/// summary of several), reading the live prompt rather than the one that was held.
fn release_held(ctx: &Ctx, mut held: hub::Held, now: u64) {
    let open = open_things(&held);
    if held.candidate.kind.is_pane_prompt() {
        if let Some(pane_id) = held.candidate.pane_id.clone() {
            held.candidate.prompt = crate::pane_prompt::detect(&ctx.registry, &pane_id, false)
                .and_then(|value| PromptInfo::from_detected(&value));
            held.candidate.heading = pane_heading(&ctx.registry, &pane_id).or(held.candidate.heading);
        }
    }
    let (verdict, deliveries) = ctx.hub.plan_held(&held, open, now);
    if verdict == Verdict::Send {
        send_deliveries(&ctx.hub, deliveries);
    } else {
        tracing::debug!(?verdict, key = %held.candidate.key, open, "held push notification not sent");
    }
}

/// Encrypt and POST each delivery (blocking; call from a worker thread).
pub fn send_deliveries(hub: &PushHub, deliveries: Vec<hub::Delivery>) {
    let subject = sender::vapid_subject();
    for delivery in deliveries {
        let request = match sender::build_request(
            &delivery.subscription,
            hub.vapid(),
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
                let _ = hub.store().lock().remove_endpoint(&delivery.subscription.endpoint);
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
