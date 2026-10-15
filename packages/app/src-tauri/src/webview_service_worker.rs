//! The desktop windows never use a service worker: it belongs to the phone PWA. 0.2.0 and older
//! registered it in the desktop webview too, and WebView2 does not route a service worker's own
//! update check through the app's asset handler, so that worker kept serving the precached UI of
//! the version that registered it, through every later update. Its folder is removed at startup,
//! after the single-instance check and before the first window, so the windows load the
//! installed version's UI. The page side (`src/lib/service-worker.ts`) never registers one again.

use std::path::{Path, PathBuf};

/// `<app local data>/EBWebView/Default/Service Worker`: registrations, script cache and the
/// worker's Cache Storage (the precache).
pub fn service_worker_dir(app_local_data: &Path) -> PathBuf {
    app_local_data.join("EBWebView").join("Default").join("Service Worker")
}

/// Removes the folder when it exists. `Ok(true)` when something was removed.
pub fn remove(app_local_data: &Path) -> std::io::Result<bool> {
    let dir = service_worker_dir(app_local_data);
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(true)
}

/// Called from `setup`; failures are logged, never fatal (the worst case is the old UI once more).
pub fn remove_at_startup(app_local_data: Option<PathBuf>) {
    let Some(dir) = app_local_data else { return };
    match remove(&dir) {
        Ok(true) => tracing::info!(path = %service_worker_dir(&dir).display(), "removed a desktop service worker left by an older version"),
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "could not remove the desktop service worker folder"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_desktop_service_worker_and_its_precache_are_removed_and_the_rest_of_the_webview_data_kept() {
        let root = std::env::temp_dir().join(format!("pm-webview-sw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sw = service_worker_dir(&root);
        std::fs::create_dir_all(sw.join("CacheStorage").join("7c0a3bc8")).unwrap();
        std::fs::write(sw.join("Database").with_extension("log"), b"x").unwrap();
        let local_storage = &root.join("EBWebView").join("Default").join("Local Storage");
        std::fs::create_dir_all(&local_storage).unwrap();
        std::fs::write(local_storage.join("leveldb"), b"keep").unwrap();

        assert!(remove(&root).unwrap());
        assert!(!sw.exists());
        assert!(local_storage.join("leveldb").exists(), "local storage (settings, drafts) stays");
        assert!(!remove(&root).unwrap(), "nothing left to remove the second time");
        let _ = std::fs::remove_dir_all(&root);
    }
}
