//! Talks to the relay for one room. This file holds the error type the publisher reads (SPEC-n32 3.8).
//! The HTTP client (`RoomClient`) is LR-8's part and is not here yet.

use pm_relay_protocol::room::RoomEnd;

/// What a relay call can fail with.
/// `Gone`: the relay says the room ended (404 or 410). `Refused`: the relay answered with an error; the text is a
/// fixed sentence, never the relay's own words. `Network`: no usable answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    Gone(Option<RoomEnd>),
    Refused(String),
    Network(String),
}
