//! JSON-RPC 2.0 framing for ACP over stdio: one JSON object per line.
//!
//! Pure functions, no I/O except [`read_capped_line`]. Three message shapes come from the agent:
//!
//! * a **response** (`id` and `result` or `error`, no `method`) to a request we sent,
//! * a **request** from the agent (`method` and `id`): it wants an answer from us,
//! * a **notification** (`method`, no `id`).
//!
//! The two id spaces are separate (Nuphos learned this the hard way: an agent request that happened to
//! carry the same number as our pending prompt was swallowed as its response and the agent hung forever).
//! A message with a `method` is never a response, whatever its id.

use std::io::{self, BufRead};

use serde_json::{json, Map, Value};

use super::error::AcpError;

/// The longest line read or written. A longer incoming line is dropped (and counted); a longer outgoing
/// one is refused before it can block the pipe.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    Request { id: Value, method: String, params: Value },
    Notification { method: String, params: Value },
    Response { id: Value, outcome: Result<Value, RpcErrorBody> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcErrorBody {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl From<RpcErrorBody> for AcpError {
    fn from(body: RpcErrorBody) -> Self {
        AcpError::Rpc { code: body.code, message: body.message, data: body.data }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseProblem {
    Malformed,
    WrongType,
    /// An object that is none of the three shapes.
    Unrecognised,
}

/// Classifies one line (without its newline).
pub fn parse_line(line: &[u8]) -> Result<Inbound, ParseProblem> {
    let text = std::str::from_utf8(line).map_err(|_| ParseProblem::Malformed)?.trim();
    if text.is_empty() {
        return Err(ParseProblem::Malformed);
    }
    let value: Value = serde_json::from_str(text).map_err(|_| ParseProblem::Malformed)?;
    let Value::Object(mut object) = value else {
        return Err(ParseProblem::WrongType);
    };
    let id = object.remove("id").filter(|id| !id.is_null());
    if let Some(Value::String(method)) = object.remove("method") {
        let params = object.remove("params").unwrap_or(Value::Null);
        return Ok(match id {
            Some(id) => Inbound::Request { id, method, params },
            None => Inbound::Notification { method, params },
        });
    }
    let Some(id) = id else {
        return Err(ParseProblem::Unrecognised);
    };
    if let Some(error) = object.remove("error") {
        return Ok(Inbound::Response { id, outcome: Err(error_body(&error)) });
    }
    match object.remove("result") {
        Some(result) => Ok(Inbound::Response { id, outcome: Ok(result) }),
        None => Err(ParseProblem::Unrecognised),
    }
}

fn error_body(error: &Value) -> RpcErrorBody {
    RpcErrorBody {
        code: error.get("code").and_then(Value::as_i64).unwrap_or(-32000),
        message: error.get("message").and_then(Value::as_str).unwrap_or("unknown error").to_string(),
        data: error.get("data").cloned(),
    }
}

/// What one read of the agent's stdout produced.
#[derive(Debug, PartialEq, Eq)]
pub enum LineRead {
    Line(Vec<u8>),
    /// The line was longer than the cap: its start was dropped and the reader is back in sync after it.
    TooLong { dropped_bytes: usize },
    Eof,
}

/// Reads up to the next `\n`, keeping at most `max` bytes. A longer line is skipped to its end, so one
/// huge frame cannot grow memory without bound and cannot desynchronise the stream.
pub fn read_capped_line<R: BufRead>(reader: &mut R, max: usize) -> io::Result<LineRead> {
    let mut line: Vec<u8> = Vec::new();
    let mut total = 0usize;
    let mut overflow = false;
    loop {
        let (consumed, done) = {
            let buffer = match reader.fill_buf() {
                Ok(buffer) => buffer,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if buffer.is_empty() {
                // End of stream. A last line without a newline still counts.
                return Ok(if overflow {
                    LineRead::TooLong { dropped_bytes: total }
                } else if line.is_empty() {
                    LineRead::Eof
                } else {
                    LineRead::Line(line)
                });
            }
            match buffer.iter().position(|b| *b == b'\n') {
                Some(at) => {
                    total += at;
                    if !overflow {
                        if line.len() + at > max {
                            overflow = true;
                            line = Vec::new();
                        } else {
                            line.extend_from_slice(&buffer[..at]);
                        }
                    }
                    (at + 1, true)
                }
                None => {
                    total += buffer.len();
                    if !overflow {
                        if line.len() + buffer.len() > max {
                            overflow = true;
                            line = Vec::new();
                        } else {
                            line.extend_from_slice(buffer);
                        }
                    }
                    (buffer.len(), false)
                }
            }
        };
        reader.consume(consumed);
        if done {
            return Ok(if overflow { LineRead::TooLong { dropped_bytes: total } } else { LineRead::Line(line) });
        }
    }
}

// ---- outgoing ----------------------------------------------------------------------------------------

fn object(method: Option<&str>, id: Option<Value>) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("jsonrpc".into(), json!("2.0"));
    if let Some(id) = id {
        map.insert("id".into(), id);
    }
    if let Some(method) = method {
        map.insert("method".into(), json!(method));
    }
    map
}

fn finish(map: Map<String, Value>) -> Result<String, AcpError> {
    let text = serde_json::to_string(&Value::Object(map)).map_err(|e| AcpError::Protocol(e.to_string()))?;
    if text.len() > MAX_LINE_BYTES {
        return Err(AcpError::Unsupported(format!("a message of {} bytes is too large to send", text.len())));
    }
    Ok(text)
}

pub fn request_line(id: u64, method: &str, params: Value) -> Result<String, AcpError> {
    let mut map = object(Some(method), Some(json!(id)));
    map.insert("params".into(), params);
    finish(map)
}

pub fn notification_line(method: &str, params: Value) -> Result<String, AcpError> {
    let mut map = object(Some(method), None);
    map.insert("params".into(), params);
    finish(map)
}

/// The answer to a request the agent sent. `id` is echoed exactly as received (number or string).
pub fn result_line(id: &Value, result: Value) -> Result<String, AcpError> {
    let mut map = object(None, Some(id.clone()));
    map.insert("result".into(), result);
    finish(map)
}

pub fn error_line(id: &Value, code: i64, message: &str) -> Result<String, AcpError> {
    let mut map = object(None, Some(id.clone()));
    map.insert("error".into(), json!({ "code": code, "message": message }));
    finish(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn a_method_with_an_id_is_a_request_never_a_response() {
        let line = br#"{"jsonrpc":"2.0","id":7,"method":"fs/read_text_file","params":{"path":"x"}}"#;
        match parse_line(line).unwrap() {
            Inbound::Request { id, method, .. } => {
                assert_eq!(id, json!(7));
                assert_eq!(method, "fs/read_text_file");
            }
            other => panic!("wrong shape: {other:?}"),
        }
    }

    #[test]
    fn notifications_responses_and_errors_are_told_apart() {
        assert!(matches!(
            parse_line(br#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#).unwrap(),
            Inbound::Notification { .. }
        ));
        assert!(matches!(
            parse_line(br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#).unwrap(),
            Inbound::Response { outcome: Ok(_), .. }
        ));
        match parse_line(br#"{"jsonrpc":"2.0","id":"a","error":{"code":-32002,"message":"nope","data":{"x":1}}}"#).unwrap() {
            Inbound::Response { id, outcome: Err(body) } => {
                assert_eq!(id, json!("a"));
                assert_eq!(body.code, -32002);
                assert_eq!(body.message, "nope");
                assert_eq!(body.data, Some(json!({"x": 1})));
            }
            other => panic!("wrong shape: {other:?}"),
        }
    }

    #[test]
    fn a_null_id_with_a_method_is_a_notification() {
        assert!(matches!(
            parse_line(br#"{"jsonrpc":"2.0","id":null,"method":"x"}"#).unwrap(),
            Inbound::Notification { .. }
        ));
    }

    #[test]
    fn junk_is_reported_not_trusted() {
        assert_eq!(parse_line(b"running 1 test"), Err(ParseProblem::Malformed));
        assert_eq!(parse_line(b""), Err(ParseProblem::Malformed));
        assert_eq!(parse_line(b"[1,2]"), Err(ParseProblem::WrongType));
        assert_eq!(parse_line(br#"{"jsonrpc":"2.0"}"#), Err(ParseProblem::Unrecognised));
        assert_eq!(parse_line(br#"{"id":1}"#), Err(ParseProblem::Unrecognised));
        assert_eq!(parse_line(&[0xff, 0xfe]), Err(ParseProblem::Malformed));
    }

    #[test]
    fn lines_are_read_whole_and_the_last_unterminated_line_counts() {
        let mut input = Cursor::new(b"one\ntwo\nthree".to_vec());
        assert_eq!(read_capped_line(&mut input, 100).unwrap(), LineRead::Line(b"one".to_vec()));
        assert_eq!(read_capped_line(&mut input, 100).unwrap(), LineRead::Line(b"two".to_vec()));
        assert_eq!(read_capped_line(&mut input, 100).unwrap(), LineRead::Line(b"three".to_vec()));
        assert_eq!(read_capped_line(&mut input, 100).unwrap(), LineRead::Eof);
    }

    #[test]
    fn an_oversized_line_is_skipped_and_the_stream_stays_in_sync() {
        let mut bytes = vec![b'x'; 5000];
        bytes.push(b'\n');
        bytes.extend_from_slice(b"after\n");
        // A tiny buffer capacity makes the line span many fills.
        let mut input = std::io::BufReader::with_capacity(16, Cursor::new(bytes));
        assert_eq!(read_capped_line(&mut input, 1000).unwrap(), LineRead::TooLong { dropped_bytes: 5000 });
        assert_eq!(read_capped_line(&mut input, 1000).unwrap(), LineRead::Line(b"after".to_vec()));
        assert_eq!(read_capped_line(&mut input, 1000).unwrap(), LineRead::Eof);
    }

    #[test]
    fn a_line_exactly_at_the_cap_is_kept() {
        let mut input = Cursor::new(b"abcd\n".to_vec());
        assert_eq!(read_capped_line(&mut input, 4).unwrap(), LineRead::Line(b"abcd".to_vec()));
        let mut input = Cursor::new(b"abcde\n".to_vec());
        assert!(matches!(read_capped_line(&mut input, 4).unwrap(), LineRead::TooLong { .. }));
    }

    #[test]
    fn outgoing_lines_have_the_right_shape_and_echo_ids_exactly() {
        let request: Value = serde_json::from_str(&request_line(5, "session/prompt", json!({"a": 1})).unwrap()).unwrap();
        assert_eq!(request, json!({"jsonrpc":"2.0","id":5,"method":"session/prompt","params":{"a":1}}));
        let note: Value = serde_json::from_str(&notification_line("session/cancel", json!({"sessionId":"s"})).unwrap()).unwrap();
        assert!(note.get("id").is_none());
        let reply: Value = serde_json::from_str(&result_line(&json!("req-1"), json!({"ok": true})).unwrap()).unwrap();
        assert_eq!(reply["id"], json!("req-1"));
        let failure: Value = serde_json::from_str(&error_line(&json!(9), -32601, "no").unwrap()).unwrap();
        assert_eq!(failure["error"]["code"], json!(-32601));
        assert!(!request_line(1, "m", json!({})).unwrap().contains('\n'));
    }

    #[test]
    fn a_message_over_the_cap_is_refused_before_it_is_queued() {
        let big = "x".repeat(MAX_LINE_BYTES + 1);
        assert!(matches!(request_line(1, "m", json!({ "t": big })), Err(AcpError::Unsupported(_))));
    }
}
