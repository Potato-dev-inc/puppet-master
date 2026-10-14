//! "Save a diagnostic file" (Settings > General): a preview of what the zip holds, a look inside one
//! file, and the save. See `crate::flight::bundle`.

use super::*;
use crate::flight::bundle::{self, Bundle, PreviewEntry, SaveReport};
use once_cell::sync::Lazy;

/// The bundle shown in the last preview, so what is saved is what was previewed.
static LAST: Lazy<Mutex<Option<Bundle>>> = Lazy::new(|| Mutex::new(None));

/// A file's text in the preview is cut here.
const SHOW_BYTES: usize = 24 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticPreview {
    pub files: Vec<PreviewEntry>,
    pub total_bytes: usize,
}

fn build_now(app: &AppHandle, state: &State<'_, AppState>) -> Bundle {
    let project = registry_get_project_path(&state.registry);
    let settings = settings_store::read_public_settings(app);
    bundle::build(bundle::gather(&project, settings))
}

#[tauri::command]
pub async fn diagnostic_preview(app: AppHandle, state: State<'_, AppState>) -> Result<DiagnosticPreview, String> {
    let bundle = build_now(&app, &state);
    let files = bundle.preview();
    let total_bytes = files.iter().map(|f| f.bytes).sum();
    *LAST.lock() = Some(bundle);
    Ok(DiagnosticPreview { files, total_bytes })
}

/// The start of one file of the last preview, for the "Show" link.
#[tauri::command]
pub async fn diagnostic_show(path: String) -> Result<String, String> {
    let guard = LAST.lock();
    let bundle = guard.as_ref().ok_or_else(|| "Open the preview first.".to_string())?;
    let file = bundle.files.iter().find(|f| f.path == path).ok_or_else(|| "That file is not in the preview.".to_string())?;
    let text = file.body.as_str();
    let mut cut = text.len().min(SHOW_BYTES);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    Ok(if cut < text.len() { format!("{}\n... (cut here; the saved file has all of it)", &text[..cut]) } else { text.to_string() })
}

/// Save the zip to `path`, leaving out the files in `exclude` (paths from the preview).
#[tauri::command]
pub async fn diagnostic_save(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    exclude: Vec<String>,
) -> Result<SaveReport, String> {
    let dest = std::path::PathBuf::from(path.trim());
    if dest.as_os_str().is_empty() || !dest.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        return Err("Choose a file name that ends in .zip.".into());
    }
    let cached = LAST.lock().take();
    let bundle = match cached {
        Some(bundle) => bundle,
        None => build_now(&app, &state),
    };
    bundle::save(&bundle, &exclude, &dest)
}
