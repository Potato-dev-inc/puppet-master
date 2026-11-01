// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Wire format. Mouse motion and heartbeats are datagrams. Everything else is a
//! length-prefixed postcard frame on a QUIC stream. Keys are USB HID usages.

#![forbid(unsafe_code)]

pub mod keys;
pub mod msg;

pub use keys::{hid_to_mac, hid_to_vk, mac_to_hid, vk_to_hid};
pub use msg::{
    decode_datagram, decode_frame, encode_datagram, encode_frame, fits_inline, ClipAnnounce,
    ClipKind, Datagram, FileAnnounce, OsKind, StreamMsg, MAX_INLINE_BYTES, PROTO_VERSION,
};
