//! The app's operation watchdog thread: every few seconds it asks `operations::sweep_now` to fail
//! runs that are well past their deadline although their own supervisor never ended them (it
//! panicked, or gave up reading the run's record). See `operations/watchdog.rs`.

use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use crate::pty::PaneRegistry;

pub(super) fn start(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle) {
    let _ = thread::Builder::new()
        .name("operation-watchdog".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_millis(crate::operations::WATCHDOG_INTERVAL_MS));
            let failed = crate::operations::sweep_now(&mut |operation| {
                super::stop_operation_worker_with_kill(&registry, &app, operation, false)
            });
            for snapshot in failed {
                tracing::warn!(
                    operation_id = %snapshot.operation_id,
                    "operation watchdog failed a run past its deadline"
                );
                let _ = super::publish_operation(&snapshot, &registry, &app);
            }
        });
}
