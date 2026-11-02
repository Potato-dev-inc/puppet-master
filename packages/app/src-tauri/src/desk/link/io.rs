// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (hop-net handshake, heartbeat task of PLAN.md N2.4, writer tasks of N2.1.)
//! One desk link, from the hello to the last byte. Everything here is I/O; every decision goes to the hub.
//!
//! - **Handshake**: the dialer opens the control stream (kind byte 1) and says hello; the other side answers. A hello
//!   whose device id is not the node the encrypted link was made with is refused. The hub then decides which of two
//!   links to the same computer survives (`p2p/session.rs`).
//! - **Writers**: control messages and motion each have their own queue and writer task. The hub only pushes; it never
//!   awaits a stream (DevHop's loop awaited writes inline, so one slow write stopped the heartbeat).
//! - **Heartbeat**: its own task, every 100 ms, independent of the hub. It also notices silence (400 ms: keys are
//!   released; 3 s: the link is closed) and a switch that was turned off (the link is closed within 100 ms).
//! - **Readers** never decide anything. A stream the peer opens after the control stream is dropped, not parsed as
//!   something else.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use desk_proto::{decode_datagram, encode_datagram, Datagram, OsKind, StreamMsg, PROTO_VERSION};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, timeout, MissedTickBehavior};

use crate::p2p::endpoint::{BiStream, Link, NodeId, HANDSHAKE_TIMEOUT};
use crate::p2p::session::{Silence, SilenceVerdict, HEARTBEAT_MS};

use super::hub::{InstallReq, Msg};
use super::wire::{read_frame, read_kind, write_frame, write_kind, CONTROL_FRAME_MAX, KIND_CONTROL};
use super::{Inner, CLOSE_DEAD, CLOSE_DUPLICATE, CLOSE_PROTOCOL, CLOSE_STOPPED};

/// How often silence and the switch are looked at.
const WATCH_MS: u64 = 25;
/// Datagrams waiting for the writer; a full queue drops motion rather than waiting.
const DG_QUEUE: usize = 256;

#[derive(Default)]
pub struct LinkShared {
    /// Milliseconds (service clock) of the last thing heard from the peer.
    pub last_rx: AtomicU64,
    pub last_beat_rx: AtomicU64,
    pub max_beat_gap: AtomicU64,
    pub beats_rx: AtomicU64,
    /// The peer's last heartbeat number, echoed back.
    pub peer_seq: AtomicU64,
}

struct Hello {
    device_id: String,
    os: OsKind,
}

pub(super) fn os_here() -> OsKind {
    if cfg!(windows) {
        OsKind::Windows
    } else if cfg!(target_os = "macos") {
        OsKind::Macos
    } else {
        OsKind::Other
    }
}

async fn handshake(inner: &Inner, link: &Arc<dyn Link>, dialed: bool) -> Option<(BiStream, Hello)> {
    let me = StreamMsg::Hello { version: PROTO_VERSION, device_id: inner.node_hex(), name: inner.self_name.clone(), os: inner.self_os };
    if dialed {
        let mut s = link.open_bi().await.ok()?;
        write_kind(&mut s.send, KIND_CONTROL).await.ok()?;
        write_frame(&mut s.send, &me).await.ok()?;
        match read_frame(&mut s.recv, CONTROL_FRAME_MAX).await.ok()?? {
            StreamMsg::HelloAck { version, device_id, os, .. } if version == PROTO_VERSION => Some((s, Hello { device_id, os })),
            _ => None,
        }
    } else {
        let mut s = link.accept_bi().await.ok()?;
        if read_kind(&mut s.recv).await.ok()? != KIND_CONTROL {
            return None;
        }
        match read_frame(&mut s.recv, CONTROL_FRAME_MAX).await.ok()?? {
            StreamMsg::Hello { version, device_id, os, .. } if version == PROTO_VERSION => {
                let reply = StreamMsg::HelloAck { version: PROTO_VERSION, device_id: inner.node_hex(), name: inner.self_name.clone(), os: inner.self_os };
                write_frame(&mut s.send, &reply).await.ok()?;
                Some((s, Hello { device_id, os }))
            }
            _ => None,
        }
    }
}

/// Runs a link until it ends. For an accepted link, called by the capability handler; for a dialed one, by the dial task.
pub(super) async fn run_link(inner: Arc<Inner>, peer: NodeId, link: Arc<dyn Link>, dialed: bool) {
    let Some(hub) = inner.hub_tx() else {
        // Mouse sharing starts when the person opens My computers on this computer (never by itself at app start).
        tracing::warn!(dialed, "desk: refused a link: mouse sharing is not started here yet, open My computers on this computer");
        link.close(CLOSE_STOPPED, "bye");
        return;
    };
    let dial_done = |ok: bool| {
        if dialed {
            let _ = hub.send(Msg::DialResult { peer, ok });
        }
    };
    let _ = hub.send(Msg::Handshaking { peer, on: true });
    let shake = timeout(HANDSHAKE_TIMEOUT, handshake(&inner, &link, dialed)).await;
    let _ = hub.send(Msg::Handshaking { peer, on: false });
    let Some((stream, hello)) = shake.ok().flatten() else {
        tracing::warn!(dialed, "desk: the hello with a computer failed (no answer in time, or a different version)");
        link.close(CLOSE_PROTOCOL, "bye");
        dial_done(false);
        return;
    };
    if hello.device_id != peer.to_hex() {
        tracing::warn!(dialed, "desk: refused a link: the hello named a different computer than the encrypted link");
        link.close(CLOSE_PROTOCOL, "bye");
        dial_done(false);
        return;
    }

    let link_id = inner.new_link_id();
    let (ctl_tx, ctl_rx) = mpsc::unbounded_channel::<StreamMsg>();
    let (dg_tx, dg_rx) = mpsc::channel::<Datagram>(DG_QUEUE);
    let (reply_tx, reply_rx) = oneshot::channel();
    let req = InstallReq { peer, link: link.clone(), link_id, dialed, os: hello.os, ctl: ctl_tx, dg: dg_tx, reply: reply_tx };
    if hub.send(Msg::Install(Box::new(req))).is_err() || !reply_rx.await.unwrap_or(false) {
        link.close(CLOSE_DUPLICATE, "bye");
        dial_done(true); // another link to this computer is already the session
        return;
    }
    dial_done(true);

    let shared = Arc::new(LinkShared::default());
    shared.last_rx.store(inner.now_ms(), Ordering::SeqCst);
    inner.register_stats(peer, shared.clone());
    run_tasks(inner, hub, link, peer, link_id, shared, stream, ctl_rx, dg_rx).await;
}

/// The tasks of one installed link, until the link ends. Then the hub is told it ended.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_tasks(
    inner: Arc<Inner>,
    hub: mpsc::UnboundedSender<Msg>,
    link: Arc<dyn Link>,
    peer: NodeId,
    link_id: u64,
    shared: Arc<LinkShared>,
    stream: BiStream,
    ctl_rx: mpsc::UnboundedReceiver<StreamMsg>,
    dg_rx: mpsc::Receiver<Datagram>,
) {
    let BiStream { send, recv } = stream;
    let tasks = [
        tokio::spawn(control_writer(send, ctl_rx, link.clone())),
        tokio::spawn(datagram_writer(link.clone(), dg_rx)),
        tokio::spawn(control_reader(inner.clone(), recv, link.clone(), peer, link_id, shared.clone())),
        tokio::spawn(datagram_reader(inner.clone(), link.clone(), peer, link_id, shared.clone())),
        tokio::spawn(extra_streams(link.clone())),
    ];

    watch(&inner, &hub, &link, peer, link_id, &shared).await;

    for t in &tasks {
        t.abort();
    }
    let _ = hub.send(Msg::Ended { peer, link_id });
}

/// The heartbeat and the watchdog, until the link closes.
pub(super) async fn watch(inner: &Arc<Inner>, hub: &mpsc::UnboundedSender<Msg>, link: &Arc<dyn Link>, peer: NodeId, link_id: u64, shared: &Arc<LinkShared>) {
    let mut beat = interval(Duration::from_millis(HEARTBEAT_MS));
    beat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut dog = interval(Duration::from_millis(WATCH_MS));
    dog.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut silence = Silence::new(inner.now_ms());
    let mut seen = shared.last_rx.load(Ordering::SeqCst);
    let mut seq: u64 = 0;
    let mut checks: u64 = 0;
    let closed = link.closed();
    tokio::pin!(closed);
    loop {
        tokio::select! {
            _ = &mut closed => return,
            _ = beat.tick() => {
                seq += 1;
                let echo = shared.peer_seq.load(Ordering::SeqCst);
                let d = Datagram::Heartbeat { seq, echo: (echo != 0).then_some(echo), sent_us: inner.now_ms() * 1000 };
                if let Ok(bytes) = encode_datagram(&d) {
                    let _ = link.send_datagram(bytes).await;
                }
            }
            _ = dog.tick() => {
                let now = inner.now_ms();
                let rx = shared.last_rx.load(Ordering::SeqCst);
                if rx != seen {
                    seen = rx;
                    silence.heard(rx);
                }
                match silence.check(now) {
                    SilenceVerdict::Fine => {}
                    SilenceVerdict::Stalled => {
                        let gap_ms = silence.quiet_for(now);
                        tracing::warn!(gap_ms, "desk: the other computer went quiet, the keys are let go");
                        let _ = hub.send(Msg::Stalled { peer, link_id, gap_ms });
                    }
                    SilenceVerdict::Dead => {
                        link.close(CLOSE_DEAD, "bye");
                        return;
                    }
                }
                checks += 1;
                // The person's switch, looked at every 100 ms: off means the link ends, whatever the peer says.
                if checks % (HEARTBEAT_MS / WATCH_MS) == 0 && !inner.input_allowed(&peer) {
                    link.close(CLOSE_STOPPED, "bye");
                    return;
                }
            }
        }
    }
}

async fn control_writer(mut send: Box<dyn tokio::io::AsyncWrite + Send + Unpin>, mut rx: mpsc::UnboundedReceiver<StreamMsg>, link: Arc<dyn Link>) {
    while let Some(msg) = rx.recv().await {
        if write_frame(&mut send, &msg).await.is_err() {
            link.close(CLOSE_PROTOCOL, "bye");
            return;
        }
    }
}

/// Sends motion and beats. A send error means this link can no longer carry motion, while heartbeats keep the link
/// looking alive: so the link is ended here, and the dialer redials it, instead of motion being dropped for good.
pub(super) async fn datagram_writer(link: Arc<dyn Link>, mut rx: mpsc::Receiver<Datagram>) {
    while let Some(d) = rx.recv().await {
        if let Ok(bytes) = encode_datagram(&d) {
            if link.send_datagram(bytes).await.is_err() {
                tracing::warn!("desk: could not send pointer moves to the other computer, the link is ended and dialed again");
                link.close(CLOSE_PROTOCOL, "bye");
                return;
            }
        }
    }
}

async fn control_reader(
    inner: Arc<Inner>,
    mut recv: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    link: Arc<dyn Link>,
    peer: NodeId,
    link_id: u64,
    shared: Arc<LinkShared>,
) {
    let Some(hub) = inner.hub_tx() else { return };
    loop {
        match read_frame(&mut recv, CONTROL_FRAME_MAX).await {
            Ok(Some(msg)) => {
                shared.last_rx.store(inner.now_ms(), Ordering::SeqCst);
                if hub.send(Msg::Frame { peer, link_id, msg }).is_err() {
                    return;
                }
            }
            // The other side ended its control stream, or sent something that is not a frame.
            other => {
                if matches!(other, Err(super::wire::WireError::Bad) | Err(super::wire::WireError::TooLarge)) {
                    // Most often two different builds: a message one of them added that the other does not know.
                    tracing::warn!("desk: the other computer sent a message this version does not understand, update both computers to the same build");
                }
                link.close(CLOSE_PROTOCOL, "bye");
                return;
            }
        }
    }
}

async fn datagram_reader(inner: Arc<Inner>, link: Arc<dyn Link>, peer: NodeId, link_id: u64, shared: Arc<LinkShared>) {
    let Some(hub) = inner.hub_tx() else { return };
    let mut last_motion: Option<u32> = None;
    while let Ok(bytes) = link.recv_datagram().await {
        let Ok(d) = decode_datagram(&bytes) else { continue };
        let now = inner.now_ms();
        shared.last_rx.store(now, Ordering::SeqCst);
        match d {
            Datagram::Heartbeat { seq, .. } => {
                let before = shared.last_beat_rx.swap(now, Ordering::SeqCst);
                if shared.beats_rx.fetch_add(1, Ordering::SeqCst) > 0 {
                    shared.max_beat_gap.fetch_max(now.saturating_sub(before), Ordering::SeqCst);
                }
                shared.peer_seq.store(seq, Ordering::SeqCst);
            }
            Datagram::Motion { seq, dx, dy, .. } => {
                // Datagrams can arrive twice or late: an old number is dropped.
                if last_motion.is_some_and(|l| (seq.wrapping_sub(l) as i32) <= 0) {
                    continue;
                }
                last_motion = Some(seq);
                if hub.send(Msg::Motion { peer, link_id, dx, dy }).is_err() {
                    return;
                }
            }
        }
    }
}

/// Any other stream on the input link is not ours to parse: it is dropped (DevHop handed every stream to the file
/// receiver). The control stream keeps working.
async fn extra_streams(link: Arc<dyn Link>) {
    while let Ok(stream) = link.accept_bi().await {
        drop(stream);
    }
}
