//! Tauri commands for Focus (`attention::focus`, SPEC n33 section 3.5). Desktop window only: no bridge route
//! and no MCP tool. Each command is a thin call into the engine (`live::engine()`); the rules, the limits and the
//! audit lines live there. The desktop always acts as `By::Desktop`.
//!
//! Errors go to the window as `CODE: message`, the same shape as the other commands, so the card can match the
//! stable code (`FOCUS_BAD_MINUTES`, `FOCUS_NOT_ON`) and show the sentence.

use crate::attention::focus::live::engine;
use crate::attention::focus::model::{By, FocusError, Settings};
use crate::attention::focus::view::FocusView;

/// `CODE: message` as the other commands send errors to the window.
fn fail(err: FocusError) -> String {
    format!("{}: {}", err.code, err.message)
}

/// The desktop view now (the Settings and the note read it on mount).
#[tauri::command]
pub async fn focus_state() -> FocusView {
    engine().view().desktop
}

/// Start a Focus of `minutes` (5 to 240), or change the length of one that is on.
#[tauri::command]
pub async fn focus_start(minutes: u32) -> Result<FocusView, String> {
    engine().start(minutes, By::Desktop).map(|view| view.desktop).map_err(fail)
}

/// Add 15 minutes to a Focus that is on.
#[tauri::command]
pub async fn focus_extend() -> Result<FocusView, String> {
    engine().extend(By::Desktop).map(|view| view.desktop).map_err(fail)
}

/// End the Focus now. Idempotent: while off this returns the off view and changes nothing.
#[tauri::command]
pub async fn focus_end() -> FocusView {
    engine().end(By::Desktop).desktop
}

/// Save the default length and the quiet switch. The default length must be 5 to 240.
#[tauri::command]
pub async fn focus_settings_set(settings: Settings) -> Result<FocusView, String> {
    engine().settings_set(settings).map(|view| view.desktop).map_err(fail)
}

/// "Got it": clear the last summary and its pending digest.
#[tauri::command]
pub async fn focus_summary_dismiss() {
    engine().summary_dismiss();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::focus::model::{CODE_BAD_MINUTES, CODE_NOT_ON};

    // These tests use the app's engine, but only on paths that refuse before anything is saved, audited or
    // emitted, so the global flag and the state file stay untouched.

    #[tokio::test]
    async fn bad_minutes_are_refused_with_the_stable_code() {
        for minutes in [0, 4, 241, u32::MAX] {
            let err = focus_start(minutes).await.unwrap_err();
            assert!(err.starts_with(&format!("{CODE_BAD_MINUTES}: ")), "{minutes} gave {err}");
        }
    }

    #[tokio::test]
    async fn focus_end_while_off_is_a_no_op_view() {
        let first = focus_end().await;
        assert!(!first.on);
        assert_eq!(first.session_id, None);
        assert_eq!(first.held_total, 0);
        assert_eq!(focus_end().await, first);
    }

    #[tokio::test]
    async fn extend_while_off_says_not_on() {
        let err = focus_extend().await.unwrap_err();
        assert!(err.starts_with(&format!("{CODE_NOT_ON}: ")), "got {err}");
    }

    #[tokio::test]
    async fn settings_with_a_bad_default_length_are_refused() {
        for minutes in [0, 4, 241] {
            let settings = Settings { default_minutes: minutes, quiet_blocking: true };
            let err = focus_settings_set(settings).await.unwrap_err();
            assert!(err.starts_with(&format!("{CODE_BAD_MINUTES}: ")), "{minutes} gave {err}");
        }
    }

    #[tokio::test]
    async fn state_is_off_before_any_start() {
        assert!(!focus_state().await.on);
    }

    #[test]
    fn errors_carry_the_code_then_the_sentence() {
        assert_eq!(fail(FocusError::bad_minutes()), "FOCUS_BAD_MINUTES: Pick between 5 and 240 minutes.");
    }
}
