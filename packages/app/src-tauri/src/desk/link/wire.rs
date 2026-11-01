// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (hop-proto framing; PLAN.md N2.2 "typed streams".)
//! Bytes on a stream. The first byte of every stream says what it is; DevHop had no such tag and treated every
//! stream the peer opened as a file transfer (PLAN.md finding 5). After the tag, frames are a little-endian `u32`
//! length and a postcard body, with a limit that fits the stream's job: the control stream carries tiny messages, so
//! a peer that announces a large control frame is cut off before anything is allocated.

use desk_proto::{decode_frame, encode_frame, StreamMsg};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The one control stream of a desk link: hello, keys, buttons, wheel, enter, leave.
pub const KIND_CONTROL: u8 = 1;
/// A clipboard stream on the clipboard link.
pub const KIND_CLIPBOARD: u8 = 2;

/// Control frames are tiny. (DevHop's limit was the clipboard's: 32 MiB.)
pub const CONTROL_FRAME_MAX: usize = 16 * 1024;
/// One clipboard chunk plus its envelope.
pub const CLIP_FRAME_MAX: usize = super::clipboard::CHUNK + 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    Closed,
    TooLarge,
    Bad,
}

pub async fn write_kind<W: AsyncWrite + Unpin + ?Sized>(w: &mut W, kind: u8) -> Result<(), WireError> {
    w.write_all(&[kind]).await.map_err(|_| WireError::Closed)
}

pub async fn read_kind<R: AsyncRead + Unpin + ?Sized>(r: &mut R) -> Result<u8, WireError> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).await.map_err(|_| WireError::Closed)?;
    Ok(b[0])
}

pub async fn write_frame<W: AsyncWrite + Unpin + ?Sized>(w: &mut W, msg: &StreamMsg) -> Result<(), WireError> {
    let bytes = encode_frame(msg).map_err(|_| WireError::TooLarge)?;
    w.write_all(&bytes).await.map_err(|_| WireError::Closed)
}

/// `Ok(None)` on a clean end of stream between frames.
pub async fn read_frame<R: AsyncRead + Unpin + ?Sized>(r: &mut R, max: usize) -> Result<Option<StreamMsg>, WireError> {
    let mut len = [0u8; 4];
    match r.read(&mut len[..1]).await {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(_) => return Err(WireError::Closed),
    }
    r.read_exact(&mut len[1..]).await.map_err(|_| WireError::Closed)?;
    let n = u32::from_le_bytes(len) as usize;
    if n > max {
        return Err(WireError::TooLarge);
    }
    let mut buf = Vec::with_capacity(4 + n);
    buf.extend_from_slice(&len);
    buf.resize(4 + n, 0);
    r.read_exact(&mut buf[4..]).await.map_err(|_| WireError::Closed)?;
    match decode_frame(&buf) {
        Ok(Some((msg, _))) => Ok(Some(msg)),
        _ => Err(WireError::Bad),
    }
}
