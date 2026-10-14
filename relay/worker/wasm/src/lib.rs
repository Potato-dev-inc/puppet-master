//! The relay's protocol checks, compiled to WebAssembly for the Cloudflare Worker variant.
//!
//! The Worker (TypeScript, `../src`) owns routing, storage, sockets and rate limits. Everything that
//! decides "is this signature, shape or binding valid" is done here by calling `relay/protocol`
//! (the same crate the app and the axum relay use), so the Worker accepts and refuses exactly what
//! the axum relay does: strict Ed25519 (no malleable signatures, no small-order keys), the same
//! signed bytes, the same size and shape rules.
//!
//! Every function takes raw bytes or strings and returns a JSON string:
//! `{"ok":true,...}` or `{"ok":false,"status":400,"error":"..."}`. The status and error text match
//! `relay/server`, because the app shows them to people.

use pm_relay_protocol as p;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

/// A refusal: the HTTP status and the message the axum relay would send.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub error: &'static str,
}

fn refuse<T>(status: u16, error: &'static str) -> Result<T, Refusal> {
    Err(Refusal { status, error })
}

fn answer(result: Result<Value, Refusal>) -> String {
    match result {
        Ok(mut value) => {
            if let Some(map) = value.as_object_mut() {
                map.insert("ok".into(), Value::Bool(true));
            }
            value.to_string()
        }
        Err(r) => json!({ "ok": false, "status": r.status, "error": r.error }).to_string(),
    }
}

// -------------------------------------------------------------------------------- core (testable)

/// A signed device request (`x-pm-auth`). The caller still checks the access list and the replay guard.
pub fn authenticate_core(header: &str, method: &str, path_and_query: &str, body: &[u8], now_ms: u64) -> Result<Value, Refusal> {
    match p::verify_request(header, method, path_and_query, body, now_ms) {
        Ok(authed) => Ok(json!({ "device": authed.device, "sig": authed.sig, "ts": authed.ts_ms })),
        Err(why) => refuse(401, why),
    }
}

/// `POST /v1/teams`.
pub fn registration_core(body: &[u8]) -> Result<Value, Refusal> {
    let Ok(reg) = serde_json::from_slice::<p::TeamRegistration>(body) else { return refuse(400, "bad registration") };
    if !reg.verify() {
        return refuse(400, "the team id, root key and access list don't match");
    }
    Ok(json!({ "team": reg.team, "root": reg.root, "salt": reg.salt, "version": reg.acl.version, "devices": reg.acl.devices }))
}

/// `PUT /v1/teams/{team}/acl`: signed by the team's root key.
pub fn acl_core(body: &[u8], team: &str, root: &str) -> Result<Value, Refusal> {
    let Ok(acl) = serde_json::from_slice::<p::SignedAcl>(body) else { return refuse(400, "bad access list") };
    if acl.team != team || !acl.verify(root) {
        return refuse(403, "not signed by the team's root key");
    }
    Ok(json!({ "version": acl.version, "devices": acl.devices }))
}

/// `PUT /v1/teams/{team}/state/{slot}`.
pub fn blob_core(body: &[u8], team: &str, slot: &str, root: &str) -> Result<Value, Refusal> {
    let Ok(blob) = serde_json::from_slice::<p::SignedBlob>(body) else { return refuse(400, "bad state") };
    if blob.team != team || blob.slot != slot || !p::SLOTS.contains(&slot) || !blob.verify(root) {
        return refuse(403, "not signed by the team's root key");
    }
    Ok(json!({ "version": blob.version }))
}

/// `POST /v1/teams/{team}/envelopes`: shape, team binding and the sender's own signature.
/// `envelope` is the canonical JSON (what the axum relay stores and hands out).
pub fn envelope_core(body: &[u8], team: &str, device: &str) -> Result<Value, Refusal> {
    let Ok(envelope) = serde_json::from_slice::<p::Envelope>(body) else { return refuse(400, "bad envelope") };
    if envelope.team != team || envelope.from != device || !envelope.verify() {
        return refuse(403, "the envelope is not signed by this device for this team");
    }
    let canonical = serde_json::to_string(&envelope).unwrap_or_default();
    Ok(json!({ "envelope": canonical }))
}

/// `POST /v1/teams/{team}/joins` (unauthenticated: only the shape is checked).
pub fn join_core(body: &[u8], team: &str) -> Result<Value, Refusal> {
    let Ok(join) = serde_json::from_slice::<p::JoinBox>(body) else { return refuse(400, "bad join request") };
    if let Err(why) = join.check_shape() {
        return refuse(400, why);
    }
    if join.team != team {
        return refuse(400, "wrong team");
    }
    let canonical = serde_json::to_string(&join).unwrap_or_default();
    Ok(json!({ "invite": join.invite, "join": canonical }))
}

/// The protocol's constants, so the Worker never restates them.
pub fn constants_core() -> Value {
    json!({
        "protocol": p::PROTOCOL,
        "authHeader": p::AUTH_HEADER,
        "maxEnvelopeBytes": p::MAX_ENVELOPE_BYTES,
        "maxJoinBytes": p::MAX_JOIN_BYTES,
        "maxStateBytes": p::MAX_STATE_BYTES,
        "mailboxTtlMs": p::MAILBOX_TTL_MS,
        "joinTtlMs": p::JOIN_TTL_MS,
        "authSkewMs": p::AUTH_SKEW_MS,
        "slots": p::SLOTS,
    })
}

// ------------------------------------------------------------------------------------ wasm glue

#[wasm_bindgen]
pub fn constants() -> String {
    answer(Ok(constants_core()))
}

#[wasm_bindgen]
pub fn is_team_id(id: &str) -> bool {
    p::is_team_id(id)
}

#[wasm_bindgen]
pub fn authenticate(header: &str, method: &str, path_and_query: &str, body: &[u8], now_ms: f64) -> String {
    answer(authenticate_core(header, method, path_and_query, body, now_ms as u64))
}

#[wasm_bindgen]
pub fn check_registration(body: &[u8]) -> String {
    answer(registration_core(body))
}

#[wasm_bindgen]
pub fn check_acl(body: &[u8], team: &str, root: &str) -> String {
    answer(acl_core(body, team, root))
}

#[wasm_bindgen]
pub fn check_blob(body: &[u8], team: &str, slot: &str, root: &str) -> String {
    answer(blob_core(body, team, slot, root))
}

#[wasm_bindgen]
pub fn check_envelope(body: &[u8], team: &str, device: &str) -> String {
    answer(envelope_core(body, team, device))
}

#[wasm_bindgen]
pub fn check_join(body: &[u8], team: &str) -> String {
    answer(join_core(body, team))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn registration(root: &SigningKey, device: &SigningKey) -> p::TeamRegistration {
        let team = p::team_id(root.verifying_key().as_bytes(), b"salt");
        p::TeamRegistration {
            team: team.clone(),
            root: p::key_id(root),
            salt: p::b64(b"salt"),
            acl: p::SignedAcl::signed(&team, 1, vec![p::key_id(device)], root),
        }
    }

    fn envelope(team: &str, device: &SigningKey) -> p::Envelope {
        let mut env = p::Envelope {
            v: 1,
            id: format!("01J{:023}", 1),
            team: team.into(),
            epoch: 1,
            from: String::new(),
            seq: 1,
            nonce: p::b64(&[1u8; 24]),
            ct: p::b64(b"sealed"),
            sig: String::new(),
        };
        env.sign_with(device);
        env
    }

    #[test]
    fn registration_is_accepted_only_when_the_team_id_binds_the_root_key() {
        let (root, device) = (key(1), key(2));
        let reg = registration(&root, &device);
        let ok = registration_core(&serde_json::to_vec(&reg).unwrap()).unwrap();
        assert_eq!(ok["team"], reg.team);
        assert_eq!(ok["version"], 1);
        assert_eq!(ok["devices"][0], p::key_id(&device));
        let mut thief = reg.clone();
        thief.root = p::key_id(&key(7));
        assert_eq!(
            registration_core(&serde_json::to_vec(&thief).unwrap()),
            Err(Refusal { status: 400, error: "the team id, root key and access list don't match" })
        );
        assert_eq!(registration_core(b"{"), Err(Refusal { status: 400, error: "bad registration" }));
    }

    #[test]
    fn acl_and_blobs_need_the_root_signature() {
        let (root, device) = (key(1), key(2));
        let team = registration(&root, &device).team;
        let root_id = p::key_id(&root);
        let acl = p::SignedAcl::signed(&team, 2, vec![p::key_id(&device)], &root);
        assert_eq!(acl_core(&serde_json::to_vec(&acl).unwrap(), &team, &root_id).unwrap()["version"], 2);
        let forged = p::SignedAcl::signed(&team, 3, vec![p::key_id(&device)], &device);
        assert_eq!(acl_core(&serde_json::to_vec(&forged).unwrap(), &team, &root_id).unwrap_err().status, 403);
        let blob = p::SignedBlob::signed(&team, "keys", 4, b"sealed", &root);
        let bytes = serde_json::to_vec(&blob).unwrap();
        assert_eq!(blob_core(&bytes, &team, "keys", &root_id).unwrap()["version"], 4);
        assert_eq!(blob_core(&bytes, &team, "roster", &root_id).unwrap_err().status, 403, "the slot in the path must match");
        let other_slot = p::SignedBlob::signed(&team, "nope", 4, b"x", &root);
        assert_eq!(blob_core(&serde_json::to_vec(&other_slot).unwrap(), &team, "nope", &root_id).unwrap_err().status, 403);
    }

    #[test]
    fn envelopes_must_be_signed_by_the_posting_device_for_this_team() {
        let (root, device) = (key(1), key(2));
        let team = registration(&root, &device).team;
        let env = envelope(&team, &device);
        let bytes = serde_json::to_vec(&env).unwrap();
        let ok = envelope_core(&bytes, &team, &p::key_id(&device)).unwrap();
        assert_eq!(serde_json::from_str::<p::Envelope>(ok["envelope"].as_str().unwrap()).unwrap(), env);
        assert_eq!(envelope_core(&bytes, &team, &p::key_id(&key(9))).unwrap_err().status, 403, "someone else posting it");
        assert_eq!(envelope_core(&bytes, "ZZZZZZZZZZZZZZZZZZZZZZZZZZ", &p::key_id(&device)).unwrap_err().status, 403);
        let mut tampered = env.clone();
        tampered.seq = 2;
        assert_eq!(envelope_core(&serde_json::to_vec(&tampered).unwrap(), &team, &p::key_id(&device)).unwrap_err().status, 403);
        assert_eq!(envelope_core(b"[]", &team, &p::key_id(&device)).unwrap_err().error, "bad envelope");
    }

    #[test]
    fn requests_are_authenticated_by_signature_and_time() {
        let device = key(2);
        let header = p::auth_header(&device, "GET", "/v1/teams/T/envelopes?after=0", 1_000_000, b"");
        let ok = authenticate_core(&header, "GET", "/v1/teams/T/envelopes?after=0", b"", 1_000_100).unwrap();
        assert_eq!(ok["device"], p::key_id(&device));
        assert_eq!(authenticate_core(&header, "GET", "/v1/teams/T/envelopes?after=1", b"", 1_000_100).unwrap_err().status, 401);
        assert_eq!(authenticate_core(&header, "GET", "/v1/teams/T/envelopes?after=0", b"", 1_000_000 + p::AUTH_SKEW_MS + 1).unwrap_err().status, 401);
    }

    #[test]
    fn joins_are_shape_checked_and_must_name_the_team() {
        let (root, device) = (key(1), key(2));
        let team = registration(&root, &device).team;
        let join = p::JoinBox { team: team.clone(), invite: "INVITE1".into(), enc: p::b64(b"e"), ct: p::b64(b"c"), at_ms: 5 };
        let bytes = serde_json::to_vec(&join).unwrap();
        assert_eq!(join_core(&bytes, &team).unwrap()["invite"], "INVITE1");
        assert_eq!(join_core(&bytes, "ZZZZZZZZZZZZZZZZZZZZZZZZZZ").unwrap_err().error, "wrong team");
        let bad = p::JoinBox { invite: "no spaces!".into(), ..join };
        assert_eq!(join_core(&serde_json::to_vec(&bad).unwrap(), &team).unwrap_err().error, "bad invite id");
    }

    #[test]
    fn answers_are_json_the_worker_can_read() {
        let refused: Value = serde_json::from_str(&answer(refuse::<Value>(403, "nope"))).unwrap();
        assert_eq!(refused, json!({ "ok": false, "status": 403, "error": "nope" }));
        let constants: Value = serde_json::from_str(&constants()).unwrap();
        assert_eq!(constants["ok"], true);
        assert_eq!(constants["mailboxTtlMs"], 7 * 24 * 60 * 60 * 1000);
    }
}
