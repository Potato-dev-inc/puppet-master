//! The protocol names (ALPNs) a Puppet Master endpoint speaks. One name per capability: the name is how a connection
//! is routed to a capability handler, and how a peer that does not speak it is refused during the handshake.
//!
//! A name is `pm/<what>/<version>`. A new version is a new name, so two builds never guess at each other's bytes.

/// Teams and spaces (Spaces D-0 uses this one).
pub const SPACES: &[u8] = b"pm/spaces/1";
/// My computers: mouse and keyboard hand-over.
pub const DESK: &[u8] = b"pm/desk/1";
/// My computers: shared clipboard.
pub const DESK_CLIPBOARD: &[u8] = b"pm/desk-clipboard/1";
/// Files between my computers.
pub const FILES: &[u8] = b"pm/files/1";

/// Every name starts with this; anything else is not ours.
pub const PREFIX: &[u8] = b"pm/";

/// Longest name accepted (a name goes into every handshake).
pub const MAX_LEN: usize = 48;

/// A name this app could own: `pm/` + printable ASCII, no spaces, bounded length.
pub fn is_valid(alpn: &[u8]) -> bool {
    alpn.starts_with(PREFIX)
        && alpn.len() > PREFIX.len()
        && alpn.len() <= MAX_LEN
        && alpn.iter().all(|b| b.is_ascii_graphic())
}

/// Whether traffic for this name may ever pass through a relay server. The rule is fixed here, not left to the
/// capability: anything under `pm/desk` moves the pointer and the keyboard, so it is direct-only, always. A relay
/// would see who talks to whom and add delay a mouse cannot take.
pub fn relay_possible(alpn: &[u8]) -> bool {
    !alpn.starts_with(b"pm/desk")
}

/// Pairing two computers. Open only while a code is on screen; direct-only by capability policy (LAN).
pub const PAIR: &[u8] = b"pm/pair/1";
