//! OpenCode API quota / rate-limit handling and pending key-swap events for wait_for_panes.

use crate::events::{PaneId, SystemEvent};
use crate::opencode::keys::{self, KeyAutomationMode, RotateTarget};
use crate::opencode::watch::{emit_opencode_worker_event, OpenCodeWorkerEvent};
use crate::pty::registry::PaneRegistry;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::AppHandle;

pub const KEY_SWAP_REQUIRED: &str = "key_swap_required";
pub const KEY_ROTATED: &str = "key_rotated";
pub const REASON_RATE_LIMITED: &str = "rate_limited";

const SCROLLBACK_RATE_LIMIT_PHRASES: &[&str] = &[
    "monthly usage limit",
    "usage limit reached",
    "rate limit",
    "quota",
    "too many requests",
    "usage cap",
];

/// Detect rate-limit / quota messages in TUI scrollback (HTTP may return 204).
pub fn scrollback_indicates_rate_limit(text: &str) -> bool {
    let lower = text.to_lowercase();
    SCROLLBACK_RATE_LIMIT_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingKeySwapInfo {
    pub kind: String,
    pub from_profile: String,
    pub to_profile: Option<String>,
}

impl From<&KeySwapEvent> for PendingKeySwapInfo {
    fn from(event: &KeySwapEvent) -> Self {
        Self {
            kind: event.kind.clone(),
            from_profile: event.from_profile.clone(),
            to_profile: event.to_profile.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeySwapEvent {
    pub kind: String,
    pub pane_id: String,
    pub from_profile: String,
    pub to_profile: Option<String>,
    pub reason: String,
    pub auto_rotated: bool,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitPlan {
    pub kind: String,
    pub from_profile: String,
    pub to_profile: Option<String>,
    pub auto_rotated: bool,
    pub should_rotate: bool,
}

pub fn plan_rate_limit_action(mode: KeyAutomationMode, active_profile: &str, other_configured: bool) -> RateLimitPlan {
    let from_profile = active_profile.to_string();
    match mode {
        KeyAutomationMode::NotifyOnly => RateLimitPlan {
            kind: KEY_SWAP_REQUIRED.into(),
            from_profile,
            to_profile: None,
            auto_rotated: false,
            should_rotate: false,
        },
        KeyAutomationMode::AutoIfBackup => {
            if other_configured {
                let to_profile = keys::toggle_profile_id(active_profile);
                RateLimitPlan {
                    kind: KEY_ROTATED.into(),
                    from_profile,
                    to_profile: Some(to_profile),
                    auto_rotated: true,
                    should_rotate: true,
                }
            } else {
                RateLimitPlan {
                    kind: KEY_SWAP_REQUIRED.into(),
                    from_profile,
                    to_profile: None,
                    auto_rotated: false,
                    should_rotate: false,
                }
            }
        },
    }
}

pub fn handle_rate_limit(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
) -> Result<KeySwapEvent, String> {
    let mode = keys::automation_mode()?;
    let restart = keys::restart_pane_on_rotate()?;
    let status = keys::status()?;
    let from_profile = status.active_profile.clone();
    let other = keys::toggle_profile_id(&from_profile);
    let other_configured = status
        .profiles
        .iter()
        .any(|profile| profile.id == other && profile.configured);

    let plan = plan_rate_limit_action(mode, &from_profile, other_configured);
    let mut to_profile = plan.to_profile.clone();

    if plan.should_rotate {
        let rotated = keys::rotate(RotateTarget::Next)?;
        to_profile = Some(rotated.active_profile.clone());
        if restart {
            crate::opencode::native::restart_native_pane(Arc::clone(registry), app, pane_id)?;
        }
    }

    let event = KeySwapEvent {
        kind: plan.kind.clone(),
        pane_id: pane_id.to_string(),
        from_profile: plan.from_profile.clone(),
        to_profile,
        reason: REASON_RATE_LIMITED.into(),
        auto_rotated: plan.auto_rotated,
        at_ms: crate::event_log::now_ms(),
    };

    store_pending_key_event(registry, pane_id, event.clone())?;

    let pane_status = registry
        .lock()
        .panes
        .get(pane_id)
        .map(|pane| pane.info.status.clone())
        .unwrap_or_else(|| "running".to_string());

    emit_opencode_worker_event(
        app,
        OpenCodeWorkerEvent {
            pane_id: pane_id.to_string(),
            event: event.kind.clone(),
            pane_status,
            serve_healthy: false,
            pending_permission_count: 0,
            pending_permission_ids: vec![],
            from_profile: Some(event.from_profile.clone()),
            to_profile: event.to_profile.clone(),
            reason: Some(event.reason.clone()),
            auto_rotated: Some(event.auto_rotated),
        },
    );

    crate::event_log::append_system_event(SystemEvent::OpenCodeKeySwap {
        pane_id: PaneId(pane_id.to_string()),
        event: event.kind.clone(),
        from_profile: event.from_profile.clone(),
        to_profile: event.to_profile.clone(),
    });

    Ok(event)
}

pub fn store_pending_key_event(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    event: KeySwapEvent,
) -> Result<(), String> {
    let mut reg = registry.lock();
    let pane = reg
        .panes
        .get_mut(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    pane.opencode_key_event = Some(event);
    Ok(())
}

pub fn take_pending_key_event(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<KeySwapEvent> {
    registry
        .lock()
        .panes
        .get_mut(pane_id)
        .and_then(|pane| pane.opencode_key_event.take())
}

pub fn pending_key_event(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<KeySwapEvent> {
    registry
        .lock()
        .panes
        .get(pane_id)
        .and_then(|pane| pane.opencode_key_event.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencode::keys::{self, KeyAutomationMode};
    use crate::pty::registry::PaneRegistry;
    use parking_lot::Mutex;
    use std::fs;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        keys::test_keys_lock()
    }

    fn with_test_keys_store<F: FnOnce()>(f: F) {
        let _guard = lock();
        let unique = format!(
            "pm-opencode-quota-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        let store_file = dir.join("opencode-key-profiles.json");
        let auth_file = dir.join("auth.json");
        fs::create_dir_all(&dir).expect("tempdir");
        std::env::set_var("PUPPET_MASTER_TEST_OPENCODE_KEYS_STORE", &store_file);
        std::env::set_var("PUPPET_MASTER_TEST_OPENCODE_AUTH", &auth_file);

        f();

        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("PUPPET_MASTER_TEST_OPENCODE_KEYS_STORE");
        std::env::remove_var("PUPPET_MASTER_TEST_OPENCODE_AUTH");
    }

    #[test]
    fn auto_if_backup_rotates_when_other_profile_configured() {
        with_test_keys_store(|| {
            keys::set_profile_api_key("a", "key-a", None).expect("set a");
            keys::set_profile_api_key("b", "key-b", None).expect("set b");
            keys::set_automation_settings(Some(KeyAutomationMode::AutoIfBackup), Some(true))
                .expect("mode");

            let plan = plan_rate_limit_action(KeyAutomationMode::AutoIfBackup, "a", true);
            assert_eq!(plan.kind, KEY_ROTATED);
            assert_eq!(plan.from_profile, "a");
            assert_eq!(plan.to_profile.as_deref(), Some("b"));
            assert!(plan.auto_rotated);
            assert!(plan.should_rotate);
        });
    }

    #[test]
    fn auto_if_backup_notifies_when_backup_missing() {
        let plan = plan_rate_limit_action(KeyAutomationMode::AutoIfBackup, "a", false);
        assert_eq!(plan.kind, KEY_SWAP_REQUIRED);
        assert!(!plan.auto_rotated);
        assert!(!plan.should_rotate);
    }

    #[test]
    fn notify_only_never_auto_rotates() {
        let plan = plan_rate_limit_action(KeyAutomationMode::NotifyOnly, "a", true);
        assert_eq!(plan.kind, KEY_SWAP_REQUIRED);
        assert!(!plan.auto_rotated);
        assert!(!plan.should_rotate);
    }

    #[test]
    fn scrollback_detects_rate_limit_phrases() {
        assert!(scrollback_indicates_rate_limit(
            "monthly usage limit reached — [retrying]"
        ));
        assert!(scrollback_indicates_rate_limit("Usage Limit Reached"));
        assert!(scrollback_indicates_rate_limit("Rate Limit exceeded"));
        assert!(scrollback_indicates_rate_limit("quota exhausted"));
        assert!(scrollback_indicates_rate_limit("Too Many Requests"));
        assert!(scrollback_indicates_rate_limit("usage cap hit"));
        assert!(!scrollback_indicates_rate_limit("session ready"));
        assert!(!scrollback_indicates_rate_limit("retrying without limit"));
    }

    #[test]
    fn pending_key_event_is_consumed_on_take() {
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        let event = KeySwapEvent {
            kind: KEY_ROTATED.into(),
            pane_id: "pane-test".into(),
            from_profile: "a".into(),
            to_profile: Some("b".into()),
            reason: REASON_RATE_LIMITED.into(),
            auto_rotated: true,
            at_ms: 1,
        };
        assert!(store_pending_key_event(&registry, "pane-test", event.clone()).is_err());

        registry
            .lock()
            .panes
            .insert("pane-test".into(), PaneRegistry::test_pane_stub("pane-test"));
        store_pending_key_event(&registry, "pane-test", event.clone()).expect("store");
        assert_eq!(pending_key_event(&registry, "pane-test"), Some(event.clone()));
        assert_eq!(take_pending_key_event(&registry, "pane-test"), Some(event));
        assert!(pending_key_event(&registry, "pane-test").is_none());
    }
}
