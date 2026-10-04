//! Process-wide bridge state: the app handle, the pane registry and the serialisation
//! mutexes shared by the HTTP routes and the operation dispatcher.

use parking_lot::Mutex;
use std::sync::Arc;
use tauri::AppHandle;

use crate::pty::PaneRegistry;

pub(super) static BRIDGE_APP: once_cell::sync::OnceCell<AppHandle> =
    once_cell::sync::OnceCell::new();
pub(super) static BRIDGE_REGISTRY: once_cell::sync::OnceCell<Arc<Mutex<PaneRegistry>>> =
    once_cell::sync::OnceCell::new();
pub(super) static RESOURCE_LOCK_SERIAL: once_cell::sync::Lazy<Mutex<()>> =
    once_cell::sync::Lazy::new(|| Mutex::new(()));
pub(super) static PANE_ASSIGNMENT_SERIAL: once_cell::sync::Lazy<Mutex<()>> =
    once_cell::sync::Lazy::new(|| Mutex::new(()));
