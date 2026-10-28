//! The Live Room relay client (SPEC-n32 3.7, 3.8). One `RoomClient` per room. It calls the relay with the
//! room's Ed25519 key (`x-pm-auth`), so only the owner's app can post frames, replace the snapshot or stop the room.
//!
//! No answer is shown to anyone as it came. Each one is read at most 4 KiB, parsed into a typed value, and any
//! failure becomes one of three errors with a fixed sentence. Transport errors are never formatted: ureq's text
//! names the URL, and the URL carries the room id. A relay's own words are never copied into an error either.

use crate::team::transport::relay::normalize_relay_url;
use ed25519_dalek::SigningKey;
use pm_relay_protocol::room::{room_id, RoomEnd, RoomFrame, RoomRegistration, RoomStatus};
use pm_relay_protocol::{auth_header, AUTH_HEADER};
use serde::Deserialize;
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long opening a connection may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long one call may take in all.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Most bytes read from any answer. The owner needs two numbers back, never more.
const MAX_ANSWER_BYTES: u64 = 4 * 1024;

const UNREACHABLE: &str = "Can't reach the relay. Check the connection and the relay address.";
const UNREADABLE: &str = "The relay sent an answer this app can't read.";
const REDIRECTED: &str = "The relay answered with a redirect. Check the relay address.";

/// Why a call to the relay did not give an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The room is over for good: stopped, expired, or unknown to the relay (404 or 410).
    Gone(Option<RoomEnd>),
    /// The relay refused the call. The text is one of our fixed sentences, never the relay's own words.
    Refused(String),
    /// The relay could not be reached, or it failed or answered badly. Worth trying again.
    Network(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Gone(_) => f.write_str("The Live Room has ended on the relay."),
            ClientError::Refused(text) | ClientError::Network(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for ClientError {}

/// The owner's calls to one room on one relay.
pub struct RoomClient {
    /// Scheme and host of the relay address, no trailing slash.
    origin: String,
    /// The path part of the relay address (`""` or `/pm`). It is signed into every call, as the team relay does.
    prefix: String,
    /// The room id, derived from the room key.
    room: String,
    /// The room key. It signs every call, so the struct does not derive `Debug`.
    key: SigningKey,
    agent: ureq::Agent,
}

impl RoomClient {
    /// A client for the room `key` names, on `relay` (`https://...`, or `http://` on this computer).
    pub fn new(relay: &str, key: SigningKey) -> Result<Self, String> {
        let base = normalize_relay_url(relay)?;
        let after_scheme = base.split_once("://").map(|(_, rest)| rest).unwrap_or("");
        let prefix = after_scheme.find('/').map(|at| after_scheme[at..].to_string()).unwrap_or_default();
        let origin = base[..base.len() - prefix.len()].to_string();
        let room = room_id(key.verifying_key().as_bytes());
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout(CALL_TIMEOUT)
            // A redirect would carry this call's signature to another address: refuse it instead.
            .redirects(0)
            .build();
        Ok(Self { origin, prefix, room, key, agent })
    }

    /// Registers the room (`POST /v1/rooms`). The registration is signed inside, so the call carries no `x-pm-auth`.
    pub fn register(&self, reg: &RoomRegistration) -> Result<(), ClientError> {
        if reg.room != self.room {
            return Err(ClientError::Refused("That registration is for another room.".into()));
        }
        let body = to_json(reg)?;
        self.send("POST", &format!("{}/v1/rooms", self.prefix), &body, false).map(|_| ())
    }

    /// Replaces the snapshot with `frame` (`PUT …/snapshot`).
    pub fn put_snapshot(&self, frame: &RoomFrame) -> Result<RoomStatus, ClientError> {
        let body = to_json(frame)?;
        status_of(&self.send("PUT", &self.room_path("/snapshot"), &body, true)?)
    }

    /// Adds one delta or state frame (`POST …/frames`).
    pub fn post_frame(&self, frame: &RoomFrame) -> Result<RoomStatus, ClientError> {
        let body = to_json(frame)?;
        status_of(&self.send("POST", &self.room_path("/frames"), &body, true)?)
    }

    /// The room's viewer count and end time (`GET …/status`).
    pub fn status(&self) -> Result<RoomStatus, ClientError> {
        status_of(&self.send("GET", &self.room_path("/status"), b"", true)?)
    }

    /// Stops the room now (`DELETE`). The relay wipes its frames and keeps a tombstone.
    pub fn delete(&self) -> Result<(), ClientError> {
        self.send("DELETE", &self.room_path(""), b"", true).map(|_| ())
    }

    fn room_path(&self, rest: &str) -> String {
        format!("{}/v1/rooms/{}{}", self.prefix, self.room, rest)
    }

    /// One call. Returns the answer's body (capped) on a 2xx, or the error for its status.
    fn send(&self, method: &str, path: &str, body: &[u8], signed: bool) -> Result<Vec<u8>, ClientError> {
        let mut req = self.agent.request(method, &format!("{}{}", self.origin, path));
        if signed {
            req = req.set(AUTH_HEADER, &auth_header(&self.key, method, path, now_ms(), body));
        }
        let sent = if body.is_empty() {
            req.call()
        } else {
            req.set("content-type", "application/json").send_bytes(body)
        };
        match sent {
            Ok(response) if (200..300).contains(&response.status()) => {
                read_capped(response.into_reader()).map_err(|_| ClientError::Network(UNREACHABLE.into()))
            }
            // Redirects are not followed (see `new`), so one arrives as a plain answer.
            Ok(_) => Err(ClientError::Refused(REDIRECTED.into())),
            Err(ureq::Error::Status(code, response)) => Err(status_error(code, &read_capped(response.into_reader()).unwrap_or_default())),
            // The transport text is never formatted: it names the URL.
            Err(ureq::Error::Transport(_)) => Err(ClientError::Network(UNREACHABLE.into())),
        }
    }
}

/// What a failed status means. Only the `ended` word of a 410 is read from the body; the rest is never shown.
fn status_error(code: u16, body: &[u8]) -> ClientError {
    match code {
        404 => ClientError::Gone(None),
        410 => ClientError::Gone(ended_in(body)),
        401 => ClientError::Refused("The relay did not accept this computer's signature.".into()),
        403 => ClientError::Refused("The relay does not accept this room's key.".into()),
        409 => ClientError::Refused("The relay already has that room or a newer frame.".into()),
        413 => ClientError::Refused("That is too large for the relay.".into()),
        429 => ClientError::Refused("The relay asks to slow down.".into()),
        400..=499 => ClientError::Refused("The relay refused the call.".into()),
        // Any 5xx (including 507, out of room memory) is the relay's trouble: try again later.
        _ => ClientError::Network("The relay had a problem. Try again later.".into()),
    }
}

#[derive(Deserialize)]
struct Ended {
    ended: RoomEnd,
}

/// `{"ended":"stopped"}` or `{"ended":"expired"}` from a 410.
fn ended_in(body: &[u8]) -> Option<RoomEnd> {
    serde_json::from_slice::<Ended>(body).ok().map(|e| e.ended)
}

/// The first `MAX_ANSWER_BYTES` of an answer. Anything past that is never read.
fn read_capped(reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_ANSWER_BYTES).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn status_of(bytes: &[u8]) -> Result<RoomStatus, ClientError> {
    serde_json::from_slice(bytes).map_err(|_| ClientError::Network(UNREADABLE.into()))
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, ClientError> {
    serde_json::to_vec(value).map_err(|_| ClientError::Refused("The call could not be written.".into()))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_room::seal::Sealer;
    use pm_relay_protocol::room::{read_hash, FrameKind, ROOM_READ_HEADER, ROOM_SEAT_HEADER};
    use pm_relay_protocol::{b64, key_id};
    use std::io::Write;
    use std::net::{SocketAddr, TcpListener};

    /// The read token of the test room (16 bytes, as the viewer link carries it).
    const TOKEN: [u8; 16] = [5; 16];
    const HOUR_MS: u64 = 60 * 60 * 1000;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn registration(key: &SigningKey) -> RoomRegistration {
        RoomRegistration::signed(key, now_ms() + HOUR_MS, 10, read_hash(&TOKEN))
    }

    /// A real relay on loopback: `spawn_local` serves the same router the relay binary does.
    fn local_relay() -> SocketAddr {
        pm_relay::spawn_local(pm_relay::Config::default()).0
    }

    /// A stand-in relay: answers one connection per entry, in order, then stops. Each answer is written
    /// whole and the connection is closed, so the caller never reuses it.
    fn canned_relay(answers: Vec<Vec<u8>>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((mut stream, _)) = listener.accept() else { return };
                // Read the request head only: these calls send no body.
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    head.push(byte[0]);
                }
                let _ = stream.write_all(&answer);
            }
        });
        addr
    }

    fn answer(status_line: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes = format!("HTTP/1.1 {status_line}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    /// Text that must never reach an error: the room id, the key, the signing header, the address.
    fn assert_no_secrets(text: &str, client: &RoomClient, key: &SigningKey) {
        let (device, seed) = (key_id(key), b64(&key.to_bytes()));
        let secrets = [client.room.as_str(), device.as_str(), seed.as_str(), AUTH_HEADER, "://", "/v1/", "127.0.0.1"];
        for secret in secrets {
            assert!(!text.contains(secret), "error text leaks {secret:?}: {text}");
        }
    }

    #[test]
    fn the_owner_flow_registers_posts_and_stops_a_room() {
        let addr = local_relay();
        let key = key(1);
        let client = RoomClient::new(&format!("http://{addr}"), key.clone()).expect("client");
        let reg = registration(&key);
        let mut sealer = Sealer::from_parts([7; 32], [1, 2, 3, 4]);
        client.register(&reg).expect("register");
        let snap = sealer.seal(&client.room, 1, FrameKind::Snap, br#"{"v":1}"#).expect("seal");
        let status = client.put_snapshot(&snap).expect("snapshot");
        assert_eq!(status, RoomStatus { viewers: 0, expires_at_ms: reg.expires_at_ms });
        let delta = sealer.seal(&client.room, 2, FrameKind::Delta, br#"{"v":1}"#).expect("seal");
        assert_eq!(client.post_frame(&delta).expect("frame").viewers, 0);
        assert_eq!(client.status().expect("status").expires_at_ms, reg.expires_at_ms);
        client.delete().expect("stop");
    }

    #[test]
    fn the_viewer_count_follows_a_viewer_who_opens_the_page() {
        let addr = local_relay();
        let key = key(1);
        let client = RoomClient::new(&format!("http://{addr}"), key.clone()).expect("client");
        client.register(&registration(&key)).expect("register");
        let snap = Sealer::from_parts([7; 32], [1, 2, 3, 4]).seal(&client.room, 1, FrameKind::Snap, b"{}").expect("seal");
        client.put_snapshot(&snap).expect("snapshot");
        let url = format!("http://{addr}/v1/rooms/{}/snapshot", client.room);
        let viewed = ureq::get(&url)
            .set(ROOM_READ_HEADER, &b64(&TOKEN))
            .set(ROOM_SEAT_HEADER, "seat-one")
            .call()
            .expect("viewer reads the snapshot");
        assert_eq!(viewed.status(), 200);
        assert_eq!(client.status().expect("status").viewers, 1);
    }

    #[test]
    fn after_a_stop_the_relay_says_gone_with_the_reason() {
        let addr = local_relay();
        let key = key(1);
        let client = RoomClient::new(&format!("http://{addr}"), key.clone()).expect("client");
        client.register(&registration(&key)).expect("register");
        let mut sealer = Sealer::from_parts([7; 32], [1, 2, 3, 4]);
        let snap = sealer.seal(&client.room, 1, FrameKind::Snap, b"{}").expect("seal");
        client.put_snapshot(&snap).expect("snapshot");
        client.delete().expect("stop");
        let late = sealer.seal(&client.room, 2, FrameKind::Delta, b"{}").expect("seal");
        assert_eq!(client.post_frame(&late), Err(ClientError::Gone(Some(RoomEnd::Stopped))));
        assert_eq!(client.status(), Err(ClientError::Gone(Some(RoomEnd::Stopped))));
        // The relay refuses a request signed in the same millisecond as one before it, so wait a moment.
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(client.delete(), Err(ClientError::Gone(Some(RoomEnd::Stopped))));
    }

    #[test]
    fn a_room_the_relay_does_not_know_is_gone() {
        let addr = local_relay();
        let key = key(2);
        let client = RoomClient::new(&format!("http://{addr}"), key.clone()).expect("client");
        let err = client.status().expect_err("no such room");
        assert_eq!(err, ClientError::Gone(None));
        assert_no_secrets(&err.to_string(), &client, &key);
    }

    #[test]
    fn a_registration_for_another_room_is_refused_before_sending() {
        let addr = local_relay();
        let client = RoomClient::new(&format!("http://{addr}"), key(1)).expect("client");
        let err = client.register(&registration(&key(2))).expect_err("another room");
        assert!(matches!(err, ClientError::Refused(_)), "{err:?}");
    }

    #[test]
    fn a_huge_answer_is_cut_and_refused_as_unreadable() {
        let addr = canned_relay(vec![answer("200 OK", &vec![b'x'; 1024 * 1024])]);
        let client = RoomClient::new(&format!("http://{addr}"), key(3)).expect("client");
        assert_eq!(client.status(), Err(ClientError::Network(UNREADABLE.into())));
    }

    #[test]
    fn a_relay_answer_is_never_copied_into_an_error() {
        let secret = "RELAY-TEXT-SECRET";
        let body = format!(r#"{{"error":"{secret}"}}"#).into_bytes();
        let addr = canned_relay(vec![
            answer("400 Bad Request", &body),
            answer("401 Unauthorized", &body),
            answer("403 Forbidden", &body),
            answer("429 Too Many Requests", &body),
            answer("500 Internal Server Error", &body),
            answer("503 Service Unavailable", &body),
        ]);
        let key = key(3);
        let client = RoomClient::new(&format!("http://{addr}"), key.clone()).expect("client");
        for _ in 0..6 {
            let err = client.status().expect_err("refused");
            assert!(!err.to_string().contains(secret), "{err}");
            assert_no_secrets(&err.to_string(), &client, &key);
        }
    }

    #[test]
    fn a_failed_connection_names_no_address_room_or_key() {
        // A port nothing listens on: bind, note it, drop it.
        let port = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port();
        let key = key(4);
        let client = RoomClient::new(&format!("http://127.0.0.1:{port}"), key.clone()).expect("client");
        let err = client.status().expect_err("nothing listens");
        assert!(matches!(err, ClientError::Network(_)), "{err:?}");
        assert!(!err.to_string().contains(&port.to_string()), "{err}");
        assert_no_secrets(&err.to_string(), &client, &key);
    }

    #[test]
    fn a_remote_address_without_https_is_refused_without_echoing_it() {
        let err = RoomClient::new("http://relay.example.com", key(5)).err().expect("refused");
        assert!(!err.contains("relay.example.com"), "{err}");
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let addr = canned_relay(vec![answer("302 Found", b"")]);
        let client = RoomClient::new(&format!("http://{addr}"), key(6)).expect("client");
        assert_eq!(client.status(), Err(ClientError::Refused(REDIRECTED.into())));
    }
}
