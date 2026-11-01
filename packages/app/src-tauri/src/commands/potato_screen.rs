//! Tauri commands of the Computer panel (the sandbox screen in a bot's chat). Desktop only: the webview is the one
//! caller that gets an iframe address with the screen password in its fragment. The bridge gives links to paired phones
//! only (`bridge/routes/potato_screen.rs`), so a program on this computer cannot ask for one.
//!
//! Errors are one string, `CODE: plain sentence`, like the other Potato Bot commands. `potato://screen` (no payload)
//! tells the panel to read `potato_screen_state` again.

use serde::Serialize;

use crate::potato::sandbox::screen::{self, Audience, Mode, ScreenError, ScreenState};

fn text(error: ScreenError) -> String {
    error.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenLinkOut {
    /// The iframe address. The password is in its fragment (`#vnc=`); show it to nobody and log it nowhere.
    pub src: String,
    pub mode: Mode,
    /// When the link runs out (epoch milliseconds).
    pub expires_ms: i64,
}

#[tauri::command]
pub fn potato_screen_state() -> ScreenState {
    screen::hub().state()
}

/// The Computer's iframe address for `bot_id`. `mode` is `view` or `control` (control only after `potato_screen_take_over`).
#[tauri::command(async)]
pub fn potato_screen_link(bot_id: String, mode: String) -> Result<ScreenLinkOut, String> {
    let mode = Mode::parse(&mode).ok_or_else(|| "SCREEN_BAD_MODE: mode is view or control".to_string())?;
    let base = crate::potato::live::bridge_base().ok_or_else(|| "SCREEN_NOT_READY: The app is still starting. Try again in a moment.".to_string())?;
    let hub = screen::hub();
    let src = hub.link(&bot_id, mode, &Audience::App, &base).map_err(text)?;
    Ok(ScreenLinkOut { src: src.into_inner(), mode, expires_ms: hub.now_ms() + screen::LINK_TTL_MS })
}

/// "Take over": the person takes the screen.
#[tauri::command]
pub fn potato_screen_take_over() -> Result<ScreenState, String> {
    screen::hub().take_over().map_err(text)
}

/// "Hand back": the person is done and gives the screen back.
#[tauri::command]
pub fn potato_screen_hand_back() -> Result<ScreenState, String> {
    screen::hub().hand_back().map_err(text)
}

/// "Skip" on a "Needs you" card.
#[tauri::command]
pub fn potato_screen_skip(card_id: String) -> Result<ScreenState, String> {
    screen::hub().skip(&card_id).map_err(text)
}

/// The panel calls this about once a minute while the person is working in the control view: the lease lasts 15 more minutes.
#[tauri::command]
pub fn potato_screen_keep() -> Result<(), String> {
    screen::hub().keep().map_err(text)
}
