//! The page's commands for per-computer keys and scrolling (see `prefs.rs`). Desktop window only, like the rest of
//! `desk/link`: nothing on the bridge, the MCP tools or the phone routes mentions them. Each says `FEATURE_OFF` while
//! Labs `desk_enabled` is off and returns the new [`LinkView`], whose devices carry a `prefs` object
//! `{alt_cmd, win_option, natural, speed, reverse_incoming, pointer_speed}`.
//!
//! `desk_link_set_keymap(node, alt_cmd, win_option)`: swap Alt/Command and Windows/Option for keys from that computer.
//! `desk_link_set_scroll(node, natural, speed, reverse_incoming)`: flip and scale the wheel, both directions.
//! `desk_link_set_pointer_speed(node, speed)`: how far that computer's pointer moves per step of this mouse (0.25 to 3).

use super::LinkView;
use crate::pairing_commands::{link_ready, node_of};

#[tauri::command]
pub async fn desk_link_set_pointer_speed(app: tauri::AppHandle, node: String, speed: f64) -> Result<LinkView, String> {
    let link = link_ready(&app)?;
    link.update_peer_prefs(&node_of(&node)?, |p| p.pointer_speed = speed)?;
    Ok(link.view())
}

#[tauri::command]
pub async fn desk_link_set_keymap(app: tauri::AppHandle, node: String, alt_cmd: bool, win_option: bool) -> Result<LinkView, String> {
    let link = link_ready(&app)?;
    link.update_peer_prefs(&node_of(&node)?, |p| {
        p.alt_cmd = alt_cmd;
        p.win_option = win_option;
    })?;
    Ok(link.view())
}

#[tauri::command]
pub async fn desk_link_set_scroll(app: tauri::AppHandle, node: String, natural: bool, speed: f64, reverse_incoming: bool) -> Result<LinkView, String> {
    let link = link_ready(&app)?;
    link.update_peer_prefs(&node_of(&node)?, |p| {
        p.natural = natural;
        p.speed = speed;
        p.reverse_incoming = reverse_incoming;
    })?;
    Ok(link.view())
}

#[cfg(test)]
mod tests {
    /// Like the other link commands: desktop window only, registered once, Labs-checked through `link_ready`.
    #[test]
    fn the_prefs_commands_are_desktop_window_commands_only() {
        let lib = include_str!("../../lib.rs");
        for name in ["desk_link_set_keymap", "desk_link_set_scroll", "desk_link_set_pointer_speed"] {
            assert_eq!(lib.matches(&format!("desk::link::prefs_commands::{name}")).count(), 1, "{name}");
        }
        let me = include_str!("prefs_commands.rs");
        let body = me.split("#[cfg(test)]").next().unwrap();
        assert_eq!(body.matches(concat!("#[tauri::", "command]")).count(), 3);
        assert_eq!(body.matches("link_ready(&app)?").count(), 3);
        assert!(!body.contains(concat!("pub ", "fn ")), "commands are async");
    }
}
