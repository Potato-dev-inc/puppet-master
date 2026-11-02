//! What this computer still needs the person to switch on before My computers can work.
//!
//! Only macOS asks: it lets a program watch and send keys and mouse moves only after the person turns on
//! Accessibility for it (System Settings > Privacy & Security > Accessibility). Windows asks for nothing, so
//! `accessibility` is `null` there ("not applicable"), never `true`.
//!
//! `desk_permissions` -> `{accessibility: true | false | null}`. Asking does not show the system prompt and
//! changes nothing; it is cheap, so the page asks again whenever the person comes back to the window.
//! Desktop window only, like the other desk commands: `FEATURE_OFF` while Labs `desk_enabled` is off.

use serde::Serialize;

#[cfg(target_os = "macos")]
mod permissions_macos;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct DeskPermissions {
    /// macOS Accessibility. `None`: this computer does not ask for it.
    pub accessibility: Option<bool>,
}

/// What this computer says right now.
pub fn current() -> DeskPermissions {
    DeskPermissions { accessibility: accessibility() }
}

#[cfg(target_os = "macos")]
fn accessibility() -> Option<bool> {
    Some(permissions_macos::accessibility_trusted())
}

#[cfg(not(target_os = "macos"))]
fn accessibility() -> Option<bool> {
    None
}

#[tauri::command]
pub async fn desk_permissions() -> Result<DeskPermissions, String> {
    if !super::global().labs() {
        return Err(super::FEATURE_OFF.into());
    }
    Ok(current())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_computer_that_asks_for_nothing_says_not_applicable() {
        assert_eq!(current(), DeskPermissions { accessibility: None });
    }

    #[test]
    fn the_page_gets_true_false_or_null() {
        for (v, text) in [(Some(true), "true"), (Some(false), "false"), (None, "null")] {
            let json = serde_json::to_string(&DeskPermissions { accessibility: v }).unwrap();
            assert_eq!(json, format!("{{\"accessibility\":{text}}}"));
        }
    }

    #[test]
    fn the_command_says_feature_off_while_labs_is_off_and_is_registered_once() {
        // The test build's desk starts with Labs off.
        let r = tauri::async_runtime::block_on(desk_permissions());
        assert!(r.unwrap_err().starts_with("FEATURE_OFF"));
        let lib = include_str!("../lib.rs");
        assert_eq!(lib.matches("desk::permissions::desk_permissions").count(), 1);
        let body = include_str!("permissions.rs").split("#[cfg(test)]").next().unwrap();
        assert_eq!(body.matches(concat!("#[tauri::", "command]")).count(), 1);
    }
}
