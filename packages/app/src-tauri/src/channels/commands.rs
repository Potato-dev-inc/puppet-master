//! Text Your Agents: commands (stub, filled in by a later task).
//!
//! Only the `Command` type lives here for now, so `policy::command_need` can match on it. The parser
//! and its tests come with the commands task (TG-5), which keeps this exact enum.

/// One thing the person sent from the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `/start CODE` (the code is optional: `/start` alone is a greeting).
    Start(Option<String>),
    Help,
    Status,
    Panes,
    /// `/to N text`: send text into the pane with that number in the last `/panes` list.
    To {
        n: usize,
        text: String,
    },
    /// `/c Name text`: send text to the companion with that name.
    Companion {
        name: String,
        text: String,
    },
    Stop,
    /// Plain text with no command: goes to the companion when there is exactly one.
    Text(String),
    /// The message was longer than the chat limit.
    TooLong,
    /// A command we do not know.
    Unknown,
}
