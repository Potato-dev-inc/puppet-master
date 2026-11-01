//! The one error type of the ACP lane.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcpError {
    /// The adapter process could not be started.
    #[error("could not start the adapter: {0}")]
    Spawn(String),
    /// The installed adapter is not one of the pinned versions (see `pin.rs`).
    #[error("adapter check failed: {0}")]
    Pin(String),
    #[error("{method} did not answer within {secs}s")]
    Timeout { method: String, secs: u64 },
    /// The adapter exited or closed its pipes.
    #[error("the adapter closed the connection")]
    Closed,
    /// The adapter stopped reading its input for longer than the write timeout (it is killed).
    #[error("the adapter did not read its input for {secs}s")]
    WriteStalled { secs: u64 },
    #[error("too many messages are waiting to be sent to the adapter")]
    QueueFull,
    /// The agent answered a request with a JSON-RPC error.
    #[error("agent error {code}: {message}")]
    Rpc { code: i64, message: String, data: Option<Value> },
    /// The agent said something this client cannot use.
    #[error("unexpected reply: {0}")]
    Protocol(String),
    #[error("a turn is already running")]
    Busy,
    #[error("the session is closed")]
    SessionClosed,
    #[error("{0}")]
    Unsupported(String),
    /// The agent needs a sign-in first. The ids are the `authMethods` it offered.
    #[error("the agent needs a sign-in")]
    AuthRequired { methods: Vec<String> },
    /// The turn went quiet for longer than the windows allow and was stopped.
    #[error("the agent stopped answering and was stopped")]
    Hung,
    /// The earlier conversation could not be restored; a fresh session is ready (policy `Refuse`).
    #[error("the earlier conversation could not be restored; a fresh session is ready")]
    HistoryLost,
    /// The conversation is open somewhere else that writes to it (the terminal): the adapter is not started.
    #[error("the conversation is open in another place that can write to it ({0})")]
    Held(String),
    #[error("no such permission request")]
    NoSuchAsk,
    #[error("option {0} was not offered by the agent")]
    BadOption(String),
}

impl AcpError {
    /// A failure that says nothing about the saved conversation: the same session id is worth a retry
    /// (the adapter was slow, died, or could not start), as opposed to "the agent does not know that session".
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            AcpError::Timeout { .. } | AcpError::Closed | AcpError::WriteStalled { .. } | AcpError::Spawn(_) | AcpError::QueueFull
        )
    }

    /// JSON-RPC code of an agent error, if this is one.
    pub fn rpc_code(&self) -> Option<i64> {
        match self {
            AcpError::Rpc { code, .. } => Some(*code),
            _ => None,
        }
    }
}

/// ACP's "authentication required" code.
pub const RPC_AUTH_REQUIRED: i64 = -32000;
/// JSON-RPC "method not found".
pub const RPC_METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC "invalid params".
pub const RPC_INVALID_PARAMS: i64 = -32602;
