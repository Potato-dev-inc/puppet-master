use puppet_master_app_lib::tool_registry;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

const SERVER_NAME: &str = "puppet-master";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const BRIDGE_PORT_FILE_ENV: &str = "PUPPET_MASTER_BRIDGE_PORT_FILE";
const DEFAULT_BRIDGE_PORT_FILE: &str = "puppet-master.bridge.port";
const APP_ID: &str = "com.puppetmaster.app";

#[derive(Debug, Clone, PartialEq, Eq)]
struct BridgeEndpoint {
    host: String,
    port: u16,
}

impl BridgeEndpoint {
    fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

fn log(message: impl AsRef<str>) {
    let _ = writeln!(io::stderr(), "[puppet-master-mcp-rs] {}", message.as_ref());
}

fn main() {
    if env::args().any(|arg| arg == "--version") {
        println!("{SERVER_NAME} {SERVER_VERSION}");
        return;
    }

    log("starting");
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                log(format!("stdin read failed: {err}"));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_json_rpc_line(&line);
        if let Some(response) = response {
            if writeln!(stdout, "{response}")
                .and_then(|_| stdout.flush())
                .is_err()
            {
                log("stdout write failed");
                break;
            }
        }
    }
}

fn handle_json_rpc_line(line: &str) -> Option<String> {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            return Some(
                json_rpc_error(
                    Value::Null,
                    -32700,
                    format!("invalid JSON-RPC request: {err}"),
                )
                .to_string(),
            );
        }
    };
    if request.get("id").is_none() {
        return None;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2024-11-05"),
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
        })),
        "tools/list" => Ok(json!({ "tools": mcp_tools() })),
        "resources/list" => Ok(json!({ "resources": tool_registry::resources() })),
        "prompts/list" => Ok(json!({ "prompts": tool_registry::prompts() })),
        "tools/call" => call_tool(request.get("params").cloned().unwrap_or_else(|| json!({}))),
        _ => Err(format!("unknown method: {method}")),
    };

    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
        Err(message) => json_rpc_error(id, -32603, message).to_string(),
    })
}

fn json_rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn mcp_tools() -> Vec<Value> {
    tool_registry::tools()
        .into_iter()
        .filter(|tool| tool.visibility.external_mcp)
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
            })
        })
        .collect()
}

fn call_tool(params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call missing params.name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = match name {
        "list_panes" => bridge_request("GET", "/panes", None)?,
        "bridge_health" => bridge_request("GET", "/health", None)?,
        "list_agent_contexts" => bridge_request("GET", "/agent-contexts", None)?,
        "read_agent_context" => read_agent_context(&args)?,
        "inspect_agent_model" => {
            let pane_id = required_string(&args, "pane_id")?;
            let lines = optional_number(&args, "lines").unwrap_or(200);
            bridge_request(
                "GET",
                &format!(
                    "/panes/{}/model?lines={lines}",
                    encode_path_segment(&pane_id)
                ),
                None,
            )?
        }
        "spawn_agent" => bridge_request("POST", "/panes", Some(args))?,
        "read_terminal_buffer" => {
            let pane_id = required_string(&args, "pane_id")?;
            let lines = optional_number(&args, "lines").unwrap_or(200);
            let response = bridge_request(
                "GET",
                &format!(
                    "/panes/{}/buffer?lines={lines}",
                    encode_path_segment(&pane_id)
                ),
                None,
            )?;
            serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|value| {
                    value
                        .get("content")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or(response)
        }
        "write_terminal_input" => {
            let pane_id = assert_worker_pane(&required_string(&args, "pane_id")?)?;
            let body = json!({
                "text": required_string(&args, "text")?,
                "append_newline": args.get("append_newline").and_then(Value::as_bool).unwrap_or(true),
            });
            bridge_request(
                "POST",
                &format!("/panes/{}/input", encode_path_segment(&pane_id)),
                Some(body),
            )?;
            "ok".to_string()
        }
        "kill_pane_process" => {
            let pane_id = assert_worker_pane(&required_string(&args, "pane_id")?)?;
            bridge_request(
                "DELETE",
                &format!("/panes/{}", encode_path_segment(&pane_id)),
                None,
            )?;
            "killed".to_string()
        }
        "create_task" => {
            let body = json!({
                "title": required_string(&args, "title")?,
                "exclusive": args.get("exclusive").and_then(Value::as_bool).unwrap_or(true),
            });
            bridge_request("POST", "/tasks", Some(body))?
        }
        "claim_task" => bridge_request(
            "POST",
            &format!(
                "/tasks/{}/claim",
                encode_path_segment(&required_string(&args, "task_id")?)
            ),
            Some(json!({
                "agent_id": required_string(&args, "agent_id")?,
                "lease_ms": args.get("lease_ms").cloned().unwrap_or(Value::Null),
            })),
        )?,
        "report_task_status" => bridge_request(
            "POST",
            &format!(
                "/tasks/{}/status",
                encode_path_segment(&required_string(&args, "task_id")?)
            ),
            Some(json!({ "status": required_string(&args, "status")? })),
        )?,
        "complete_task" => bridge_request(
            "POST",
            &format!(
                "/tasks/{}/complete",
                encode_path_segment(&required_string(&args, "task_id")?)
            ),
            Some(json!({
                "agent_id": required_string(&args, "agent_id")?,
                "evidence": args.get("evidence").and_then(Value::as_str).unwrap_or_default(),
            })),
        )?,
        "list_tasks" => bridge_request("GET", "/tasks", None)?,
        "acquire_resource_lock" => bridge_request("POST", "/locks", Some(args))?,
        "release_resource_lock" => bridge_request("POST", "/locks/release", Some(args))?,
        "build_context_pack" => bridge_request("POST", "/context-packs", Some(args))?,
        "read_session_context" => bridge_request("GET", "/session/context", None)?,
        "update_session_context" => bridge_request("PATCH", "/session/context", Some(args))?,
        "set_pane_role" => {
            let pane_id = required_string(&args, "pane_id")?;
            bridge_request(
                "POST",
                &format!("/panes/{}/role", encode_path_segment(&pane_id)),
                Some(args),
            )?
        }
        "read_pane_digest" => {
            let pane_id = required_string(&args, "pane_id")?;
            bridge_request(
                "GET",
                &format!("/panes/{}/digest", encode_path_segment(&pane_id)),
                None,
            )?
        }
        "update_pane_digest" => {
            let pane_id = required_string(&args, "pane_id")?;
            bridge_request(
                "POST",
                &format!("/panes/{}/digest", encode_path_segment(&pane_id)),
                Some(args),
            )?
        }
        "delegate_task" => bridge_request("POST", "/delegate-task", Some(args))?,
        "read_orchestrator_state" => bridge_request("GET", "/orchestrator/state", None)?,
        "update_orchestrator_state" => {
            bridge_request("PATCH", "/orchestrator/state", Some(args))?
        }
        _ => return Err(format!("unknown tool: {name}")),
    };

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

fn read_agent_context(args: &Value) -> Result<String, String> {
    if let Some(pane_id) = args.get("pane_id").and_then(Value::as_str) {
        return bridge_request(
            "GET",
            &format!("/panes/{}/agent-context", encode_path_segment(pane_id)),
            None,
        );
    }
    let agent_type = required_string(args, "agent_type")?;
    let contexts = bridge_request("GET", "/agent-contexts", None)?;
    let value: Value = serde_json::from_str(&contexts)
        .map_err(|err| format!("bridge returned invalid agent contexts JSON: {err}"))?;
    let context = value
        .as_array()
        .and_then(|contexts| {
            contexts.iter().find(|context| {
                context.get("agent_type").and_then(Value::as_str) == Some(&agent_type)
            })
        })
        .ok_or_else(|| format!("unknown agent_type: {agent_type}"))?;
    serde_json::to_string_pretty(context).map_err(|err| format!("serialize agent context: {err}"))
}

fn bridge_request(method: &str, path: &str, body: Option<Value>) -> Result<String, String> {
    let endpoint = read_bridge_endpoint()?;
    let body_text = body.map(|value| value.to_string()).unwrap_or_default();
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .map_err(|err| format!("bridge_down: {} ({err})", endpoint.base_url()))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let request = format!(
        "{method} {path} HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Connection: close\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        endpoint.host,
        endpoint.port,
        body_text.as_bytes().len(),
        body_text
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("bridge write failed: {err}"))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|err| format!("bridge read failed: {err}"))?;
    parse_http_response(&response)
}

fn parse_http_response(response: &[u8]) -> Result<String, String> {
    let marker = b"\r\n\r\n";
    let header_end = response
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or_else(|| "bridge returned invalid HTTP response".to_string())?;
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| "bridge returned invalid HTTP status".to_string())?;
    let body = String::from_utf8_lossy(&response[header_end + marker.len()..]).to_string();
    if status >= 400 {
        return Err(format!("bridge returned {status}: {body}"));
    }
    Ok(body)
}

fn read_bridge_endpoint() -> Result<BridgeEndpoint, String> {
    let candidates = bridge_port_candidates();
    read_bridge_endpoint_from_candidates(&candidates)
}

fn read_bridge_endpoint_from_candidates(candidates: &[PathBuf]) -> Result<BridgeEndpoint, String> {
    let mut last_error = String::new();
    for candidate in candidates {
        match fs::read_to_string(candidate) {
            Ok(raw) => return parse_bridge_endpoint(&raw),
            Err(err) => last_error = format!("{}: {err}", candidate.display()),
        }
    }
    Err(format!(
        "bridge_down: Puppet Master bridge port file not found (tried: {}). Last error: {last_error}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn bridge_port_candidates() -> Vec<PathBuf> {
    if let Ok(path) = env::var(BRIDGE_PORT_FILE_ENV) {
        return vec![PathBuf::from(path)];
    }
    vec![
        PathBuf::from(DEFAULT_BRIDGE_PORT_FILE),
        default_app_data_bridge_port_file(),
    ]
}

fn default_app_data_bridge_port_file() -> PathBuf {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    } else if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    } else {
        home.join(".local")
            .join("share")
            .join(APP_ID)
            .join(DEFAULT_BRIDGE_PORT_FILE)
    }
}

fn parse_bridge_endpoint(raw: &str) -> Result<BridgeEndpoint, String> {
    let trimmed = raw.trim();
    let (host, port_text) = trimmed
        .split_once(':')
        .map(|(host, port)| (if host.is_empty() { "127.0.0.1" } else { host }, port))
        .unwrap_or(("127.0.0.1", trimmed));
    let port = port_text
        .parse::<u16>()
        .map_err(|err| format!("invalid bridge port file value {trimmed:?}: {err}"))?;
    Ok(BridgeEndpoint {
        host: host.to_string(),
        port,
    })
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing required string argument: {key}"))
}

fn optional_number(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn assert_worker_pane(pane_id: &str) -> Result<String, String> {
    if pane_id.starts_with("puppet-master-orchestrator-") {
        return Err(format!("refusing to target orchestrator pane: {pane_id}"));
    }
    Ok(pane_id.to_string())
}

fn encode_path_segment(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_and_port() {
        assert_eq!(
            parse_bridge_endpoint("127.0.0.1:17321\n").unwrap(),
            BridgeEndpoint {
                host: "127.0.0.1".to_string(),
                port: 17321,
            }
        );
    }

    #[test]
    fn parses_port_only() {
        assert_eq!(
            parse_bridge_endpoint("17321").unwrap(),
            BridgeEndpoint {
                host: "127.0.0.1".to_string(),
                port: 17321,
            }
        );
    }

    #[test]
    fn missing_port_file_returns_bridge_down_error() {
        let missing = PathBuf::from("definitely-missing-puppet-master-port-file");
        let err = read_bridge_endpoint_from_candidates(&[missing]).unwrap_err();
        assert!(err.contains("bridge_down"));
    }

    #[test]
    fn initialize_returns_server_info() {
        let response = handle_json_rpc_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(
            value
                .pointer("/result/serverInfo/name")
                .and_then(Value::as_str),
            Some(SERVER_NAME)
        );
    }

    #[test]
    fn tools_list_contains_bridge_health() {
        let response =
            handle_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
                .unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        let tools = value
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .unwrap();
        assert!(tools
            .iter()
            .any(|tool| tool.get("name").and_then(Value::as_str) == Some("bridge_health")));
    }

    #[test]
    fn tools_list_contains_session_context_tools() {
        let response =
            handle_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
                .unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        let tools = value
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .unwrap();
        assert!(tools.iter().any(|tool| {
            tool.get("name").and_then(Value::as_str) == Some("read_session_context")
        }));
        assert!(tools
            .iter()
            .any(|tool| tool.get("name").and_then(Value::as_str) == Some("delegate_task")));
    }

    #[test]
    fn resources_list_contains_session() {
        let response = handle_json_rpc_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        let resources = value
            .pointer("/result/resources")
            .and_then(Value::as_array)
            .unwrap();
        assert!(resources.iter().any(|resource| {
            resource.get("uri").and_then(Value::as_str) == Some("puppet-master://session")
        }));
    }

    #[test]
    fn prompts_list_contains_status_check() {
        let response =
            handle_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list","params":{}}"#)
                .unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        let prompts = value
            .pointer("/result/prompts")
            .and_then(Value::as_array)
            .unwrap();
        assert!(prompts
            .iter()
            .any(|prompt| prompt.get("name").and_then(Value::as_str) == Some("status_check")));
    }

    #[test]
    fn notification_has_no_response() {
        assert!(handle_json_rpc_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#
        )
        .is_none());
    }

    #[test]
    fn rejects_orchestrator_pane_targets() {
        assert!(assert_worker_pane("puppet-master-orchestrator-123").is_err());
        assert!(assert_worker_pane("codex-123").is_ok());
    }
}
