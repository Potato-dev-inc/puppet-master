use super::model::{OperationError, OperationSnapshot};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(super) static DELEGATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
pub(super) static UPDATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static TASK_INDEX_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CHANGE: OnceLock<(Mutex<u64>, parking_lot::Condvar)> = OnceLock::new();
static RUNTIME_ID: OnceLock<String> = OnceLock::new();
#[cfg(test)]
thread_local! {
    static TEST_INDEX_OVERRIDE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn change() -> &'static (Mutex<u64>, parking_lot::Condvar) {
    CHANGE.get_or_init(|| (Mutex::new(0), parking_lot::Condvar::new()))
}
pub(super) fn current_runtime_id() -> &'static str {
    RUNTIME_ID.get_or_init(|| Uuid::new_v4().to_string())
}
pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn root(project: &str) -> Result<PathBuf, OperationError> {
    let path = Path::new(project);
    if !path.is_absolute() {
        return Err(OperationError::new(
            "INVALID_PROJECT_PATH",
            "project_path must be absolute",
            false,
        ));
    }
    Ok(path.join(".puppet-master").join("operations"))
}
pub(super) fn path_for(project: &str, id: &str) -> Result<PathBuf, OperationError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(OperationError::new(
            "INVALID_OPERATION_ID",
            "invalid operation_id",
            false,
        ));
    }
    Ok(root(project)?.join(format!("{id}.json")))
}
pub(super) fn persist(op: &OperationSnapshot) -> Result<(), OperationError> {
    let path = path_for(&op.project_path, &op.operation_id)?;
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent)
        .map_err(|e| OperationError::io_at("create operations dir", parent, e))?;
    update_indexes(&op.project_path, op.task_id.as_deref(), &op.operation_id)?;
    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    fs::write(
        &temp,
        serde_json::to_vec_pretty(op).map_err(OperationError::io)?,
    )
    .map_err(|e| OperationError::io_at("write operation file", &temp, e))?;
    replace_file(&temp, &path)?;
    let (lock, cv) = change();
    {
        let mut generation = lock.lock();
        *generation = generation.wrapping_add(1);
    }
    cv.notify_all();
    Ok(())
}
fn update_indexes(
    project: &str,
    task_id: Option<&str>,
    operation_id: &str,
) -> Result<(), OperationError> {
    let _guard = TASK_INDEX_LOCK.get_or_init(|| Mutex::new(())).lock();
    let dir = index_dir()?;
    let project_path = dir.join("operation-project-index.json");
    let task_path = dir.join("operation-task-index.json");
    let mut projects: Vec<String> = match fs::read(&project_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(OperationError::io)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(OperationError::io(error)),
    };
    // Validate every relevant index before changing either one. A corrupt task index must not
    // partially update the project index before the operation write fails.
    let mut tasks: HashMap<String, (String, String)> = if task_id.is_some() {
        match fs::read(&task_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(OperationError::io)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(OperationError::io(error)),
        }
    } else {
        HashMap::new()
    };
    if !projects.iter().any(|existing| existing == project) {
        projects.push(project.to_string());
    }
    if let Some(task_id) = task_id {
        tasks.insert(
            task_id.to_string(),
            (project.to_string(), operation_id.to_string()),
        );
    }
    atomic_json(&project_path, &projects)?;
    if task_id.is_some() {
        atomic_json(&task_path, &tasks)?;
    }
    Ok(())
}
fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<(), OperationError> {
    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    fs::write(
        &temp,
        serde_json::to_vec(value).map_err(OperationError::io)?,
    )
    .map_err(|e| OperationError::io_at("write operation index", &temp, e))?;
    replace_file(&temp, path)
}
pub(super) fn indexed_projects() -> Result<Vec<String>, OperationError> {
    let path = index_dir()?.join("operation-project-index.json");
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(OperationError::io(err)),
    };
    serde_json::from_slice(&bytes).map_err(OperationError::io)
}
pub(super) fn task_index_path() -> Result<PathBuf, OperationError> {
    Ok(index_dir()?.join("operation-task-index.json"))
}
fn index_dir() -> Result<PathBuf, OperationError> {
    #[cfg(test)]
    {
        if let Some(dir) = TEST_INDEX_OVERRIDE.with(|value| value.borrow().clone()) {
            fs::create_dir_all(&dir).map_err(OperationError::io)?;
            return Ok(dir);
        }
        static TEST_INDEX_DIR: OnceLock<PathBuf> = OnceLock::new();
        let dir = TEST_INDEX_DIR
            .get_or_init(|| {
                std::env::temp_dir().join(format!(
                    "puppet-master-operation-index-{}",
                    std::process::id()
                ))
            })
            .clone();
        fs::create_dir_all(&dir).map_err(OperationError::io)?;
        Ok(dir)
    }
    #[cfg(not(test))]
    {
        crate::app_paths::ensure_app_data_dir()
            .map_err(|e| OperationError::io_at("create app data dir", Path::new("<app data dir>"), e))
    }
}
#[cfg(test)]
pub(super) fn set_test_index_dir(path: Option<PathBuf>) {
    TEST_INDEX_OVERRIDE.with(|value| *value.borrow_mut() = path);
}
#[cfg(not(windows))]
fn replace_file(temp: &Path, path: &Path) -> Result<(), OperationError> {
    fs::rename(temp, path).map_err(|e| {
        let _ = fs::remove_file(temp);
        OperationError::io(e)
    })
}
#[cfg(windows)]
fn replace_file(temp: &Path, path: &Path) -> Result<(), OperationError> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Another process (or a polling reader without FILE_SHARE_DELETE) can hold the target open
    // for a few milliseconds; a swallowed failure here would freeze an operation's state.
    let mut attempt = 0_u64;
    loop {
        let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) };
        if ok != 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        // 5 = ACCESS_DENIED, 32 = SHARING_VIOLATION, 33 = LOCK_VIOLATION
        let transient = matches!(e.raw_os_error(), Some(5 | 32 | 33));
        attempt += 1;
        if !transient || attempt >= 25 {
            let _ = fs::remove_file(temp);
            return Err(OperationError::io_at("replace operation file", path, e));
        }
        std::thread::sleep(std::time::Duration::from_millis(10 * attempt.min(10)));
    }
}

#[cfg(all(test, windows))]
mod replace_tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn replace_waits_out_a_briefly_exclusive_holder() {
        let dir = std::env::temp_dir().join(format!("pm-replace-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("op.json");
        let temp = dir.join("op.json.tmp");
        fs::write(&target, "old").unwrap();
        fs::write(&temp, "new").unwrap();
        // share_mode(0) makes MoveFileExW fail with ACCESS_DENIED until the handle is dropped.
        let holder = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            drop(holder);
        });
        replace_file(&temp, &target).unwrap();
        release.join().unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        let _ = fs::remove_dir_all(dir);
    }
}
pub(super) fn read(path: &Path) -> Result<OperationSnapshot, OperationError> {
    let raw = fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            OperationError::new("OPERATION_NOT_FOUND", "operation not found", false)
        } else {
            OperationError::io(e)
        }
    })?;
    serde_json::from_slice(&raw).map_err(OperationError::io)
}
pub(super) fn find_key(
    project: &str,
    key: &str,
) -> Result<Option<OperationSnapshot>, OperationError> {
    let entries = match fs::read_dir(root(project)?) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OperationError::io(e)),
    };
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|e| e == "json") {
            let op = read(&entry.path())?;
            if op.idempotency_key == key {
                return Ok(Some(op));
            }
        }
    }
    Ok(None)
}
pub(super) fn bump(op: &mut OperationSnapshot) -> Result<(), OperationError> {
    op.revision = op.revision.saturating_add(1);
    op.worker.event_cursor = op.worker.event_cursor.max(op.revision);
    op.updated_at_ms = now_ms();
    persist(op)
}
