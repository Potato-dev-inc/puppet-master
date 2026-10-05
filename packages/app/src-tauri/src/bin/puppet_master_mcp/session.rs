//! Per-process MCP session identity and the starting tool mode.

use super::*;

/// Starting tool mode requested through `--mode` or `PUPPET_MASTER_MODE`.
pub(crate) static INITIAL_MODE: std::sync::OnceLock<tool_registry::McpMode> =
    std::sync::OnceLock::new();
/// True once the bridge knows this connection's mode (either the starting mode or a `set_mode` call).
pub(crate) static INITIAL_MODE_SYNCED: AtomicBool = AtomicBool::new(false);

/// Opt-in stable identity: a reconnecting MCP process that sets the same value reuses its session
/// id, so runs it started stay owned without take_over. Left unset, every process gets a fresh id.
pub(crate) const CLIENT_ID_ENV: &str = "PUPPET_MASTER_CLIENT_ID";

pub(crate) fn session_id_from(client_id: Option<String>) -> String {
    let stable: String = client_id
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .take(64)
        .collect();
    if stable.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        format!("client-{stable}")
    }
}

thread_local! { pub(crate) static MCP_SESSION_ID: RefCell<Option<String>> = const { RefCell::new(None) }; }
pub(crate) fn set_mcp_session_id(id: String) {
    MCP_SESSION_ID.with(|slot| *slot.borrow_mut() = Some(id));
}
pub(crate) fn current_mcp_session_id() -> Option<String> {
    MCP_SESSION_ID.with(|slot| slot.borrow().clone())
}

/// Reads the starting tool mode from `--mode <mode>` / `--mode=<mode>` (wins) or the
/// `PUPPET_MASTER_MODE` value. Hosts that read the tool list once and ignore
/// `notifications/tools/list_changed` use this to launch in the catalog they need.
/// Returns `Ok(None)` when nothing was requested.
pub(crate) fn initial_mode_from(
    args: &[String],
    env_value: Option<String>,
) -> Result<Option<tool_registry::McpMode>, String> {
    let mut requested: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--mode=") {
            requested = Some(value.to_string());
        } else if arg == "--mode" {
            let value = iter
                .next()
                .ok_or_else(|| "--mode requires agent, shell, or both".to_string())?;
            requested = Some(value.clone());
        }
    }
    let raw = match requested.or(env_value) {
        Some(value) => value,
        None => return Ok(None),
    };
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(None);
    }
    tool_registry::McpMode::parse(&value)
        .map(Some)
        .ok_or_else(|| format!("invalid mode {raw:?}: expected agent, shell, or both"))
}

/// Tells the bridge which mode this connection started in. The local catalog is set at
/// startup; the bridge enforces the mode per session too, so it must learn it before the
/// first tool call. Retried on later calls while the bridge is unreachable.
pub(crate) fn ensure_initial_mode_synced() {
    let Some(mode) = INITIAL_MODE.get().copied() else {
        return;
    };
    if INITIAL_MODE_SYNCED.load(Ordering::Acquire) {
        return;
    }
    match bridge_request("POST", "/mcp/mode", Some(json!({ "mode": mode.as_str() }))) {
        Ok(_) => INITIAL_MODE_SYNCED.store(true, Ordering::Release),
        Err(err) => log(format!(
            "could not apply starting mode {} to the bridge yet: {err}",
            mode.as_str()
        )),
    }
}

pub(crate) fn ensure_tool_mode(name: &str) -> Result<(), String> {
    if let Some(session) = current_mcp_session_id() {
        let mode = mcp_sessions::mode_for(&session);
        if !tool_registry::tool_visible_in_mode(name, mode) {
            let requested =
                if tool_registry::tool_visible_in_mode(name, tool_registry::McpMode::Agent) {
                    "agent"
                } else {
                    "shell"
                };
            return Err(json!({"code":"MODE_MISMATCH","message":format!("{name} is unavailable in {} mode",mode.as_str()),"recoverable":false,"context":{"current_mode":mode.as_str(),"switch_with":{"mode":requested}}}).to_string());
        }
    }
    Ok(())
}
