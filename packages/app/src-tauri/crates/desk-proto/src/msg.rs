// From desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT. Changed for Puppet Master: renamed crate.
//! Postcard messages. A stream frame is a little-endian `u32` length followed
//! by the postcard body. Datagrams are the postcard body alone.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PROTO_VERSION: u16 = 1;
/// Text, rich text, and images travel inline. Larger payloads are refused (R8).
pub const MAX_INLINE_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAME_BYTES: usize = MAX_INLINE_BYTES + 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtoError {
    #[error("postcard: {0}")]
    Codec(String),
    #[error("frame is larger than the inline cap")]
    TooLarge,
    #[error("length prefix is truncated")]
    Truncated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OsKind {
    Windows,
    Macos,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipKind {
    Text,
    Html,
    Rtf,
    Png,
    Files,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipAnnounce {
    pub kind: ClipKind,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAnnounce {
    pub rel_path: String,
    pub bytes: u64,
    pub is_dir: bool,
    pub mtime_ms: i64,
}

/// Unreliable. Motion stays off the stream so a file copy cannot delay it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Datagram {
    Motion {
        seq: u32,
        dx: i32,
        dy: i32,
        sent_us: u64,
    },
    Heartbeat {
        seq: u64,
        /// Seq this peer is acknowledging, if any.
        echo: Option<u64>,
        sent_us: u64,
    },
}

/// Reliable control, clipboard, and drag signalling.
/// File bytes themselves use `hop-transfer` on a separate low-priority stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamMsg {
    Hello {
        version: u16,
        device_id: String,
        name: String,
        os: OsKind,
    },
    HelloAck {
        version: u16,
        device_id: String,
        name: String,
        os: OsKind,
    },
    /// The peer is not paired, so the input in this message was ignored (R7).
    InputRejected,
    Key {
        hid: u16,
        down: bool,
    },
    Button {
        button: u8,
        down: bool,
    },
    Wheel {
        dx: i32,
        dy: i32,
    },
    ReleaseAll,
    Enter {
        monitor_id: String,
        nx_milli: u16,
        ny_milli: u16,
    },
    Leave,
    ClipboardAnnounce {
        token: u64,
        items: Vec<ClipAnnounce>,
    },
    ClipboardChunk {
        token: u64,
        kind: ClipKind,
        seq: u32,
        last: bool,
        bytes: Vec<u8>,
    },
    Layout {
        bytes: Vec<u8>,
    },
    DragStart {
        token: u64,
        files: Vec<FileAnnounce>,
    },
    DragCancel {
        token: u64,
    },
    DragDrop {
        token: u64,
    },
    PairConfirm,
    PairReject,
    /// The peer pasted. Start streaming token's files.
    TransferRequest {
        token: u64,
    },
    /// Puppet Master addition (kept last so older numbers do not move): the controlled computer's pointer pushed
    /// back through the edge it came in by. `t_milli` is how far along that edge (0..=1000) the pointer was.
    Return {
        t_milli: u16,
    },
    /// Puppet Master addition (kept last): the same, with the exact arrival point on the other computer, in that
    /// computer's local space (the box around its screens starts at 0, 0).
    ReturnAt {
        x: i32,
        y: i32,
    },
    /// Puppet Master addition (kept last): the computer the pointer tried to cross to said no, in plain words the
    /// person can act on ("Puppet Master needs Accessibility access ..."). Always followed by `Leave`. Never carries
    /// keys, clipboard contents or anything the person typed.
    Refused {
        reason: String,
    },
}

pub fn fits_inline(len: usize) -> bool {
    len <= MAX_INLINE_BYTES
}

pub fn encode_datagram(msg: &Datagram) -> Result<Vec<u8>, ProtoError> {
    postcard::to_allocvec(msg).map_err(|e| ProtoError::Codec(e.to_string()))
}

pub fn decode_datagram(bytes: &[u8]) -> Result<Datagram, ProtoError> {
    postcard::from_bytes(bytes).map_err(|e| ProtoError::Codec(e.to_string()))
}

pub fn encode_frame(msg: &StreamMsg) -> Result<Vec<u8>, ProtoError> {
    let body = postcard::to_allocvec(msg).map_err(|e| ProtoError::Codec(e.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(ProtoError::TooLarge);
    }
    let len = u32::try_from(body.len()).map_err(|_| ProtoError::TooLarge)?;
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decode one frame from the front of `buf`.
/// `Ok(None)` means the buffer does not hold a full frame yet.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(StreamMsg, usize)>, ProtoError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(ProtoError::TooLarge);
    }
    if buf.len() < 4 + len {
        return Ok(None);
    }
    let body = &buf[4..4 + len];
    let msg = postcard::from_bytes(body).map_err(|e| ProtoError::Codec(e.to_string()))?;
    Ok(Some((msg, 4 + len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<StreamMsg> {
        vec![
            StreamMsg::Hello {
                version: PROTO_VERSION,
                device_id: "dev-1".into(),
                name: "Desk".into(),
                os: OsKind::Windows,
            },
            StreamMsg::HelloAck {
                version: PROTO_VERSION,
                device_id: "dev-2".into(),
                name: "Laptop".into(),
                os: OsKind::Macos,
            },
            StreamMsg::InputRejected,
            StreamMsg::Key {
                hid: 0x06,
                down: true,
            },
            StreamMsg::Button {
                button: 0,
                down: false,
            },
            StreamMsg::Wheel { dx: 0, dy: -120 },
            StreamMsg::ReleaseAll,
            StreamMsg::Enter {
                monitor_id: "m2".into(),
                nx_milli: 300,
                ny_milli: 800,
            },
            StreamMsg::Leave,
            StreamMsg::ClipboardAnnounce {
                token: 7,
                items: vec![ClipAnnounce {
                    kind: ClipKind::Text,
                    bytes: 5,
                }],
            },
            StreamMsg::ClipboardChunk {
                token: 7,
                kind: ClipKind::Png,
                seq: 0,
                last: true,
                bytes: vec![1, 2, 3, 4],
            },
            StreamMsg::Layout { bytes: vec![9, 9] },
            StreamMsg::DragStart {
                token: 3,
                files: vec![FileAnnounce {
                    rel_path: "a/b.txt".into(),
                    bytes: 12,
                    is_dir: false,
                    mtime_ms: 1_700_000_000_000,
                }],
            },
            StreamMsg::DragCancel { token: 3 },
            StreamMsg::DragDrop { token: 3 },
            StreamMsg::PairConfirm,
            StreamMsg::PairReject,
            StreamMsg::TransferRequest { token: 3 },
            StreamMsg::Return { t_milli: 640 },
            StreamMsg::ReturnAt { x: -1280, y: 300 },
            StreamMsg::Refused { reason: "Accessibility is off".into() },
        ]
    }

    #[test]
    fn every_stream_message_round_trips() {
        for msg in samples() {
            let frame = encode_frame(&msg).unwrap();
            let (back, n) = decode_frame(&frame).unwrap().unwrap();
            assert_eq!(n, frame.len());
            assert_eq!(back, msg);
        }
    }

    #[test]
    fn return_was_added_last_so_older_numbers_did_not_move() {
        // postcard numbers variants in order: the first variant is 0, and `Return` must come after `TransferRequest`.
        let first = encode_frame(&StreamMsg::Hello { version: 1, device_id: String::new(), name: String::new(), os: OsKind::Windows }).unwrap();
        assert_eq!(first[4], 0);
        let transfer = encode_frame(&StreamMsg::TransferRequest { token: 0 }).unwrap();
        let ret = encode_frame(&StreamMsg::Return { t_milli: 0 }).unwrap();
        assert_eq!(ret[4], transfer[4] + 1);
        let ret_at = encode_frame(&StreamMsg::ReturnAt { x: 0, y: 0 }).unwrap();
        assert_eq!(ret_at[4], ret[4] + 1);
        let refused = encode_frame(&StreamMsg::Refused { reason: String::new() }).unwrap();
        assert_eq!(refused[4], ret_at[4] + 1);
    }

    #[test]
    fn a_partial_frame_waits() {
        let frame = encode_frame(&StreamMsg::PairConfirm).unwrap();
        assert!(decode_frame(&frame[..3]).unwrap().is_none());
        assert!(decode_frame(&frame[..frame.len() - 1]).unwrap().is_none());
    }

    #[test]
    fn datagrams_round_trip() {
        let motion = Datagram::Motion {
            seq: 4,
            dx: -3,
            dy: 8,
            sent_us: 99,
        };
        let bytes = encode_datagram(&motion).unwrap();
        assert_eq!(decode_datagram(&bytes).unwrap(), motion);
        let beat = Datagram::Heartbeat {
            seq: 10,
            echo: Some(9),
            sent_us: 50,
        };
        assert_eq!(
            decode_datagram(&encode_datagram(&beat).unwrap()).unwrap(),
            beat
        );
    }

    #[test]
    fn inline_cap_is_32_mib() {
        assert!(fits_inline(MAX_INLINE_BYTES));
        assert!(!fits_inline(MAX_INLINE_BYTES + 1));
        assert_eq!(MAX_INLINE_BYTES, 32 * 1024 * 1024);
    }

    #[test]
    fn version_constant_is_one() {
        assert_eq!(PROTO_VERSION, 1);
    }
}
