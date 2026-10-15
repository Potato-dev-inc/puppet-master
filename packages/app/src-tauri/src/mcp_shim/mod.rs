//! Stdio MCP shim: speaks JSON-RPC on stdio and maps every tool call to a loopback bridge request.
//!
//! The shim owns no state beyond the MCP session id and starting mode. Modules: `stdio` (JSON-RPC
//! loop), `call` + `progress` (tool calls, long-poll waits), `dispatch/` (tool name -> bridge
//! request, per family), `bridge_http` + `endpoint` (talking to the app), `recovery` (retry and
//! timeout payloads), `shell_hint`, `session`, `args`.

use puppet_master_app_lib::{mcp_sessions, tool_registry};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

mod args;
mod ask_user;
mod bridge_http;
mod call;
mod dispatch;
mod endpoint;
pub(crate) mod hook;
mod notices;
mod progress;
mod recovery;
mod session;
mod shell_hint;
mod stdio;

pub(crate) use args::*;
pub(crate) use bridge_http::*;
pub(crate) use call::*;
pub(crate) use endpoint::*;
pub(crate) use notices::*;
pub(crate) use progress::*;
pub(crate) use recovery::*;
pub(crate) use session::*;
pub(crate) use shell_hint::*;
pub(crate) use stdio::run;

const SERVER_NAME: &str = "puppet-master";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const BRIDGE_PORT_FILE_ENV: &str = "PUPPET_MASTER_BRIDGE_PORT_FILE";
const DEFAULT_BRIDGE_PORT_FILE: &str = "puppet-master.bridge.port";
const APP_ID: &str = "com.puppetmaster.app";
const MODE_ENV: &str = "PUPPET_MASTER_MODE";
/// Default foreground wait for run_agent, followup_task, and wait_agents (matches bridge routes).
const DEFAULT_AGENT_WAIT_MS: u64 = 120_000;

fn log(message: impl AsRef<str>) {
    let _ = writeln!(io::stderr(), "[puppet-master-mcp-rs] {}", message.as_ref());
}
