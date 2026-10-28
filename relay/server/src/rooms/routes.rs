//! The Live Room routes (SPEC-n32 §3.7). Owner calls carry `x-pm-auth` signed by the room key; viewer
//! calls are GETs with the read token in `x-pm-room-read` and a seat in `x-pm-room-seat`. Bodies are
//! limited per route before they are parsed. The static viewer (`/r/`) is the page built from
//! `relay/viewer`, served with the strict CSP of SPEC-n32 §3.9.

use super::{is_delta_kind, RoomError, REGISTRATION_BYTES};
use crate::{refuse, Refusal, Relay, Reply, HOUR_MS};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pm_relay_protocol::room::{
    read_hash, FrameKind, RoomBatch, RoomFrame, RoomRegistration, MAX_ROOM_FRAME_BYTES, MAX_ROOM_SNAPSHOT_BYTES,
    ROOM_READ_HEADER, ROOM_SEAT_HEADER,
};
use pm_relay_protocol::{unb64, verify_request, AUTH_HEADER};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

const MINUTE_MS: u64 = 60 * 1000;

/// The viewer page's policy (SPEC-n32 §3.9). Scripts and styles come from the relay itself only.
const VIEWER_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; \
    img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

const VIEWER_HTML: &str = include_str!("../../../viewer/dist/viewer.html");
const VIEWER_JS: &str = include_str!("../../../viewer/dist/viewer.js");
const VIEWER_CSS: &str = include_str!("../../../viewer/dist/viewer.css");

/// The room routes. Merged into the relay's router by `crate::router`.
pub fn routes() -> Router<Arc<Relay>> {
    Router::new()
        .route("/v1/rooms", post(register).layer(DefaultBodyLimit::max(REGISTRATION_BYTES)))
        .route("/v1/rooms/{room}", delete(stop_room).layer(DefaultBodyLimit::max(0)))
        .route(
            "/v1/rooms/{room}/snapshot",
            put(put_snapshot)
                .layer(DefaultBodyLimit::max(MAX_ROOM_SNAPSHOT_BYTES))
                .get(get_snapshot)
                .layer(DefaultBodyLimit::max(0)),
        )
        .route(
            "/v1/rooms/{room}/frames",
            post(post_frame).layer(DefaultBodyLimit::max(MAX_ROOM_FRAME_BYTES)).get(get_frames).layer(DefaultBodyLimit::max(0)),
        )
        .route("/v1/rooms/{room}/status", get(get_status).layer(DefaultBodyLimit::max(0)))
        .route("/r/", get(viewer_page))
        .route("/r/viewer.js", get(viewer_js))
        .route("/r/viewer.css", get(viewer_css))
        // Room answers are never cached (a stopped room must not be served from a cache).
        .layer(axum::middleware::map_response(no_store))
}

async fn no_store(mut res: Response) -> Response {
    res.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    res
}

/// A room refusal as the HTTP answer: a status and a short body. Relay text is fixed, never echoed.
fn http(e: RoomError) -> Refusal {
    match e {
        RoomError::Missing => refuse(StatusCode::NOT_FOUND, "not found"),
        RoomError::Ended(end) => (StatusCode::GONE, Json(json!({ "ended": end }))),
        RoomError::Full => (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "full": true }))),
        RoomError::SlowDown => refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"),
        RoomError::Conflict(why) => refuse(StatusCode::CONFLICT, why),
        RoomError::Busy => refuse(StatusCode::SERVICE_UNAVAILABLE, "the relay is busy, try again shortly"),
        RoomError::Storage => refuse(StatusCode::INSUFFICIENT_STORAGE, "the relay is out of room memory, try again later"),
    }
}

/// Checks an owner call in this order: the signature, the room's key, then the replay guard (so a
/// stranger's requests are never remembered). The body is read before this, and parsed after.
fn owner_call(relay: &Relay, room: &str, method: &str, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Result<(), Refusal> {
    let header = headers
        .get(AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| refuse(StatusCode::UNAUTHORIZED, "signed request required"))?;
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let now = relay.room_now();
    let authed = verify_request(header, method, path, body, now).map_err(|why| refuse(StatusCode::UNAUTHORIZED, why))?;
    let key = relay.rooms.owner_key(room, now).map_err(http)?;
    if key != authed.device {
        return Err(refuse(StatusCode::FORBIDDEN, "not the room's key"));
    }
    relay
        .seen_sigs
        .lock()
        .unwrap()
        .first_use(&authed.sig, now, relay.limits.max_seen_sigs)
        .map_err(|(status, why)| refuse(status, why))?;
    Ok(())
}

/// What a viewer call carries after its checks: the hash of its read token, its seat and its address.
struct ViewerCall {
    token_hash: String,
    seat: String,
    addr: String,
}

/// Checks a viewer call's shape and rate. A missing or wrong-size token is the same 404 as an unknown
/// room, so a probe learns nothing.
fn viewer_call(relay: &Relay, peer: SocketAddr, headers: &HeaderMap) -> Result<ViewerCall, Refusal> {
    let addr = relay.addr(peer, headers);
    if !relay.hit(format!("room-view:{addr}"), MINUTE_MS, relay.limits.viewer_requests_per_minute) {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let token = headers
        .get(ROOM_READ_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(unb64)
        .filter(|t| t.len() == 16)
        .ok_or_else(|| http(RoomError::Missing))?;
    let seat = headers
        .get(ROOM_SEAT_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        .ok_or_else(|| refuse(StatusCode::BAD_REQUEST, "bad seat"))?;
    Ok(ViewerCall { token_hash: read_hash(&token), seat: seat.to_string(), addr })
}

async fn register(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Reply {
    let addr = relay.addr(peer, &headers);
    if !relay.hit(format!("room-reg:{addr}"), HOUR_MS, relay.limits.rooms_per_address_per_hour) {
        return Err(refuse(StatusCode::TOO_MANY_REQUESTS, "slow down"));
    }
    let reg: RoomRegistration = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad registration"))?;
    let now = relay.room_now();
    reg.verify(now).map_err(|why| refuse(StatusCode::BAD_REQUEST, why))?;
    relay.rooms.register(&reg, now, &relay.limits).map_err(http)?;
    Ok((StatusCode::CREATED, Json(json!({ "room": reg.room }))).into_response())
}

async fn put_snapshot(
    State(relay): State<Arc<Relay>>,
    Path(room): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Reply {
    owner_call(&relay, &room, "PUT", &uri, &headers, &body)?;
    let frame: RoomFrame = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad frame"))?;
    if frame.kind != FrameKind::Snap {
        return Err(refuse(StatusCode::BAD_REQUEST, "a snapshot must be kind snap"));
    }
    frame.check_shape().map_err(|why| refuse(StatusCode::BAD_REQUEST, why))?;
    if frame.room != room {
        return Err(refuse(StatusCode::BAD_REQUEST, "the frame is for another room"));
    }
    let status = relay.rooms.put_snapshot(&room, frame, body.len(), relay.room_now(), &relay.limits).map_err(http)?;
    Ok(Json(status).into_response())
}

async fn post_frame(
    State(relay): State<Arc<Relay>>,
    Path(room): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Reply {
    owner_call(&relay, &room, "POST", &uri, &headers, &body)?;
    let frame: RoomFrame = serde_json::from_slice(&body).map_err(|_| refuse(StatusCode::BAD_REQUEST, "bad frame"))?;
    if !is_delta_kind(frame.kind) {
        return Err(refuse(StatusCode::BAD_REQUEST, "frames are kind delta or state"));
    }
    frame.check_shape().map_err(|why| refuse(StatusCode::BAD_REQUEST, why))?;
    if frame.room != room {
        return Err(refuse(StatusCode::BAD_REQUEST, "the frame is for another room"));
    }
    let size = body.len();
    let status = relay.rooms.push(&room, frame, size, relay.room_now(), &relay.limits).map_err(http)?;
    Ok(Json(status).into_response())
}

// The GET and DELETE handlers below read their body too (limit 0): a body is refused with 413.
async fn stop_room(
    State(relay): State<Arc<Relay>>,
    Path(room): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Reply {
    owner_call(&relay, &room, "DELETE", &uri, &headers, &body)?;
    relay.rooms.end(&room, relay.room_now()).map_err(http)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn get_status(
    State(relay): State<Arc<Relay>>,
    Path(room): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Reply {
    owner_call(&relay, &room, "GET", &uri, &headers, &body)?;
    let status = relay.rooms.status(&room, relay.room_now()).map_err(http)?;
    Ok(Json(status).into_response())
}

async fn get_snapshot(
    State(relay): State<Arc<Relay>>,
    Path(room): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    _body: Bytes,
) -> Reply {
    let call = viewer_call(&relay, peer, &headers)?;
    let snapshot = relay
        .rooms
        .view(&room, &call.token_hash, &call.seat, &call.addr, relay.room_now(), &relay.limits, |r| r.snapshot_frame())
        .map_err(http)?;
    Ok(match snapshot {
        Some(frame) => Json(frame).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

/// Frames after `after`, waiting up to `wait` seconds (at most the relay's long-poll limit) for new ones.
async fn get_frames(
    State(relay): State<Arc<Relay>>,
    Path(room): Path<String>,
    uri: Uri,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    _body: Bytes,
) -> Reply {
    let call = viewer_call(&relay, peer, &headers)?;
    let after = query_u64(&uri, "after").unwrap_or(0);
    let wait = query_u64(&uri, "wait").unwrap_or(0).min(relay.limits.max_wait_s);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait);
    let mut waited = false;
    loop {
        // Enabled before the check, so a frame that lands between the check and the wait wakes us.
        let notify = relay.rooms.notifier(&room).unwrap_or_else(|| Arc::new(Notify::new()));
        let waiter = notify.notified();
        tokio::pin!(waiter);
        waiter.as_mut().enable();
        let read = relay
            .rooms
            .view(&room, &call.token_hash, &call.seat, &call.addr, relay.room_now(), &relay.limits, |r| r.frames_after(after));
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match read {
            Ok(frames) if !frames.is_empty() || left.is_zero() => {
                let next = frames.last().map(|f| f.seq).unwrap_or(after);
                return Ok(Json(RoomBatch { frames, next, ended: None }).into_response());
            }
            Ok(_) => {}
            // Ended while this call waited: the answer says so, the viewer stops.
            Err(RoomError::Ended(end)) if waited => {
                return Ok(Json(RoomBatch { frames: Vec::new(), next: after, ended: Some(end) }).into_response());
            }
            Err(e) => return Err(http(e)),
        }
        waited = true;
        let _ = tokio::time::timeout(left.min(Duration::from_secs(1)), waiter).await;
    }
}

fn query_u64(uri: &Uri, key: &str) -> Option<u64> {
    uri.query()?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.parse().ok()).flatten()
    })
}

fn viewer_file(body: &'static str, content_type: &'static str) -> Response {
    (
        [
            ("content-type", content_type),
            ("content-security-policy", VIEWER_CSP),
            ("referrer-policy", "no-referrer"),
            ("x-content-type-options", "nosniff"),
            ("cross-origin-opener-policy", "same-origin"),
        ],
        body,
    )
        .into_response()
}

async fn viewer_page() -> Response {
    viewer_file(VIEWER_HTML, "text/html; charset=utf-8")
}

async fn viewer_js() -> Response {
    viewer_file(VIEWER_JS, "text/javascript; charset=utf-8")
}

async fn viewer_css() -> Response {
    viewer_file(VIEWER_CSS, "text/css; charset=utf-8")
}
