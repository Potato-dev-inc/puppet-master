//! The Potato Bot send with attached files (its own file, so the main command file stays as it is). Desktop only,
//! like the other Potato commands: the files are saved in the bot's own folder and the brain gets one text message.
//! Errors are one string, `CODE: plain sentence`.

use crate::potato::attachments::{self, Incoming};
use crate::potato::live;
use crate::potato::service::SendOutcome;
use crate::potato::store::{self, StoreError};

fn failed(error: StoreError) -> String {
    error.to_string()
}

/// One message with files. The files are checked and saved first (none is kept when one is refused), then the
/// message goes out like any other send. A send that fails removes the files again.
#[tauri::command]
pub async fn potato_send_files(bot_id: String, text: String, files: Vec<Incoming>) -> Result<SendOutcome, String> {
    let service = live::service().ok_or_else(|| "POTATO_NOT_READY: Potato Bot is not ready yet. Try again in a moment.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = service.root().to_path_buf();
        let known = store::load_bots(&root).map_err(failed)?.iter().any(|b| b.id == bot_id);
        if !known {
            return Err("POTATO_BOT_NOT_FOUND: No bot has that name.".to_string());
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let saved = attachments::save_all(&root, &bot_id, &files, stamp).map_err(failed)?;
        let sent = attachments::compose(&text, &saved).and_then(|message| service.send(&bot_id, &message));
        if sent.is_err() {
            attachments::forget(&root, &bot_id, &saved);
        }
        sent.map_err(failed)
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?
}
