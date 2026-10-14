//! Sealing and opening team messages (COWORK-PLAN §8.2), and the checks that run on every message
//! **before** anything reaches `store::ingest`.
//!
//! Sending: the payload (a `TeamEvent`, or a "the team list changed" nudge) is encrypted with the
//! current epoch key into an [`Envelope`] and signed by this device.
//!
//! Receiving, in this order, and a message that fails any step is dropped:
//! 1. the envelope's shape, and that it is for my team;
//! 2. the sender is an active device in the signed roster (removed devices are refused);
//! 3. the device's signature over the header and ciphertext;
//! 4. I hold that epoch's key; decryption (the header is the associated data);
//! 5. the replay guard (envelope id) and gap detection (per-device counter);
//! 6. the payload parses; an event must name the same team, member and device that signed it, and
//!    the member's role must allow the event's kind.
//!
//! Only then is a [`Verified`] made, and `ingest` takes nothing else. GitHub facts (which come
//! from the person's own `gh`, not from a teammate) are the one other way to make one
//! ([`Verified::local_fact`]).
//!
//! Team keys travel sealed per device with HPKE in a [`KeyGrant`] (the relay's `keys` slot); the
//! roster travels encrypted with the epoch key (the `roster` slot). Both are signed by the root.

use super::crypto::{self, b64, hpke_open, hpke_seal, open_with_key, unb64, KemSecret, SigningKey};
use super::event::{new_event_id, TeamEvent, TeamEventKind};
use super::keystore::KeyStore;
use super::roster::{Role, Roster, SignedOp};
use pm_relay_protocol::{Envelope, SignedBlob};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What travels inside an envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Payload {
    Event { event: TeamEvent },
    /// The admin changed the roster or the keys: fetch the `keys` and `roster` slots.
    RosterChanged { seq: u64 },
}

/// A teammate's event that passed every check. `store::ingest` takes only this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    event: TeamEvent,
    /// The member who signed it (`None` for a local GitHub fact).
    member: Option<String>,
}

impl Verified {
    pub fn event(&self) -> &TeamEvent {
        &self.event
    }

    pub fn member(&self) -> Option<&str> {
        self.member.as_deref()
    }

    pub fn into_event(self) -> TeamEvent {
        self.event
    }

    /// A fact this app learned itself from GitHub through the person's own `gh` login. Still
    /// untrusted text (titles come from anyone who can open a PR), so `ingest` validates it too.
    pub fn local_fact(event: TeamEvent) -> Option<Self> {
        (event.source == super::event::EventSource::Github).then_some(Self { event, member: None })
    }

    #[cfg(test)]
    pub fn for_test(event: TeamEvent) -> Self {
        let member = Some(event.actor.member.clone());
        Self { event, member }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    Event(Verified),
    RosterChanged(u64),
    /// My own message, echoed back.
    Mine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    Malformed,
    OtherTeam,
    UnknownDevice,
    BadSignature,
    NoKey,
    Undecryptable,
    Replay,
    BadPayload,
    Impersonation,
    NotAllowed,
}

impl Reject {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed envelope",
            Self::OtherTeam => "another team",
            Self::UnknownDevice => "sender is not on the team",
            Self::BadSignature => "bad signature",
            Self::NoKey => "no key for that epoch",
            Self::Undecryptable => "could not decrypt",
            Self::Replay => "already seen",
            Self::BadPayload => "bad payload",
            Self::Impersonation => "event names someone other than its sender",
            Self::NotAllowed => "the sender's role does not allow it",
        }
    }
}

// ------------------------------------------------------------------------------ gap detection

/// Missing counters from one device.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSeq {
    pub max: u64,
    /// Counters below `max` not received yet (bounded).
    #[serde(default)]
    pub missing: BTreeSet<u64>,
    /// Missing counters that did not fit in `missing`.
    #[serde(default)]
    pub overflow: u64,
}

const MAX_MISSING_PER_DEVICE: usize = 500;

/// Per-device counters: replay guard by envelope id, and gaps ("some updates may be missing").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeqTracker {
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceSeq>,
    /// Envelope id → forget after (ms).
    #[serde(default)]
    pub seen: BTreeMap<String, u64>,
}

impl SeqTracker {
    /// Record one authentic envelope. `Err(Replay)` if it was seen before.
    pub fn observe(&mut self, device: &str, seq: u64, envelope_id: &str, now_ms: u64) -> Result<(), Reject> {
        self.seen.retain(|_, until| *until > now_ms);
        if self.seen.contains_key(envelope_id) {
            return Err(Reject::Replay);
        }
        self.seen.insert(envelope_id.to_string(), now_ms + pm_relay_protocol::MAILBOX_TTL_MS * 2);
        let entry = self.devices.entry(device.to_string()).or_default();
        if entry.max == 0 {
            // First message from this device: earlier ones predate me.
            entry.max = seq;
        } else if seq > entry.max {
            for missing in entry.max + 1..seq {
                if entry.missing.len() < MAX_MISSING_PER_DEVICE {
                    entry.missing.insert(missing);
                } else {
                    entry.overflow += 1;
                }
            }
            entry.max = seq;
        } else if !entry.missing.remove(&seq) {
            // Not a known gap and not new: the sender's counter restarted (a reinstall). Start
            // counting from here; the id check above already stops true replays.
            entry.max = seq;
            entry.missing.clear();
            entry.overflow = 0;
        }
        Ok(())
    }

    /// How many messages from teammates are known to be missing.
    pub fn missing(&self) -> u64 {
        self.devices.values().map(|d| d.missing.len() as u64 + d.overflow).sum()
    }

    /// "I've seen the warning": forget the known gaps.
    pub fn clear_gaps(&mut self) {
        for d in self.devices.values_mut() {
            d.missing.clear();
            d.overflow = 0;
        }
    }
}

// --------------------------------------------------------------------------------- the session

/// My identity on one team plus its verified roster: seals what I send, opens what I receive.
#[derive(Debug, Clone)]
pub struct Session {
    pub roster: Roster,
    pub keys: KeyStore,
}

impl Session {
    pub fn new(roster: Roster, keys: KeyStore) -> Self {
        Self { roster, keys }
    }

    pub fn team(&self) -> &str {
        &self.roster.team
    }

    pub fn my_device(&self) -> Result<String, String> {
        Ok(crypto::key_id(&self.keys.device_sign()?))
    }

    pub fn my_member(&self) -> Result<String, String> {
        Ok(crypto::key_id(&self.keys.member()?))
    }

    /// Seal a payload with the current epoch key, signed by this device.
    pub fn seal(&self, payload: &Payload, seq: u64, now_ms: u64) -> Result<Envelope, String> {
        let epoch = self.roster.epoch;
        let key = self.keys.epoch_key(self.team(), epoch)?.ok_or("This computer doesn't have the team key yet.")?;
        let device = self.keys.device_sign()?;
        let id = match payload {
            Payload::Event { event } => event.id.clone(),
            Payload::RosterChanged { .. } => new_event_id(now_ms),
        };
        let mut env = Envelope {
            v: 1,
            id,
            team: self.team().to_string(),
            epoch,
            from: crypto::key_id(&device),
            seq,
            nonce: String::new(),
            ct: String::new(),
            sig: String::new(),
        };
        let plaintext = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
        // The nonce is part of the header (the associated data), so it is picked first.
        let nonce = crypto::random_bytes::<24>();
        env.nonce = b64(&nonce);
        let ct = crypto::seal_with_key_and_nonce(&key, &env.header_bytes(), &plaintext, nonce).map_err(|e| e.to_string())?;
        env.ct = b64(&ct);
        env.sign_with(&device);
        Ok(env)
    }

    /// Run every check on an envelope from the wire (see the module doc for the order).
    pub fn open(&self, env: &Envelope, tracker: &mut SeqTracker, now_ms: u64) -> Result<Opened, Reject> {
        if env.check_shape().is_err() {
            return Err(Reject::Malformed);
        }
        if env.team != self.team() {
            return Err(Reject::OtherTeam);
        }
        if self.my_device().ok().as_deref() == Some(env.from.as_str()) {
            return Ok(Opened::Mine);
        }
        let Some((member, device)) = self.roster.device(&env.from) else {
            return Err(Reject::UnknownDevice);
        };
        if !env.verify() {
            return Err(Reject::BadSignature);
        }
        let key = self.keys.epoch_key(self.team(), env.epoch).ok().flatten().ok_or(Reject::NoKey)?;
        let nonce = unb64(&env.nonce).ok_or(Reject::Malformed)?;
        let ct = unb64(&env.ct).ok_or(Reject::Malformed)?;
        let plaintext = open_with_key(&key, &nonce, &env.header_bytes(), &ct).map_err(|_| Reject::Undecryptable)?;
        tracker.observe(&device.id, env.seq, &env.id, now_ms)?;
        let payload: Payload = serde_json::from_slice(&plaintext).map_err(|_| Reject::BadPayload)?;
        match payload {
            Payload::RosterChanged { seq } => {
                if !self.roster.is_admin(&member.id) {
                    return Err(Reject::NotAllowed);
                }
                Ok(Opened::RosterChanged(seq))
            }
            Payload::Event { event } => {
                if event.id != env.id || event.team != env.team {
                    return Err(Reject::Impersonation);
                }
                if event.actor.member != member.id || event.actor.device != device.id {
                    return Err(Reject::Impersonation);
                }
                if event.source == super::event::EventSource::Github || event.kind.is_github_fact() {
                    // GitHub facts ("checks passed", "merged", "closed") are learned from GitHub by
                    // each member, never from a teammate, whatever the event says its source is.
                    return Err(Reject::NotAllowed);
                }
                let needs = match event.kind {
                    TeamEventKind::Answer => Role::Approve,
                    _ => Role::Watch,
                };
                if !member.has(needs) {
                    return Err(Reject::NotAllowed);
                }
                Ok(Opened::Event(Verified { member: Some(member.id.clone()), event }))
            }
        }
    }
}

// ------------------------------------------------------------------------- key grants and roster

/// One epoch key sealed to one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedKey {
    pub device: String,
    pub enc: String,
    pub ct: String,
}

/// The `keys` slot: the current epoch key, sealed to every active device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyGrant {
    pub team: String,
    pub epoch: u64,
    pub roster_seq: u64,
    pub sealed: Vec<SealedKey>,
}

fn grant_info(team: &str, epoch: u64, device: &str) -> Vec<u8> {
    format!("pm-epoch-key-v1\n{team}\n{epoch}\n{device}").into_bytes()
}

/// Seal `key` (epoch `roster.epoch`) to every active device of the roster.
pub fn make_grant(roster: &Roster, key: &[u8; 32]) -> Result<KeyGrant, String> {
    let mut sealed = Vec::new();
    for device in roster.all_devices() {
        let (enc, ct) = hpke_seal(&device.kem, &grant_info(&roster.team, roster.epoch, &device.id), key).map_err(|e| e.to_string())?;
        sealed.push(SealedKey { device: device.id.clone(), enc: b64(&enc), ct: b64(&ct) });
    }
    Ok(KeyGrant { team: roster.team.clone(), epoch: roster.epoch, roster_seq: roster.seq(), sealed })
}

/// My key from a grant, if one was sealed to my device.
pub fn open_grant(grant: &KeyGrant, device_id: &str, kem: &KemSecret) -> Option<[u8; 32]> {
    let mine = grant.sealed.iter().find(|s| s.device == device_id)?;
    let key = hpke_open(kem, &unb64(&mine.enc)?, &grant_info(&grant.team, grant.epoch, device_id), &unb64(&mine.ct)?).ok()?;
    key.try_into().ok()
}

/// The `keys` slot blob for a grant, signed by the root. Version: roster seq + 1.
pub fn grant_blob(grant: &KeyGrant, root: &SigningKey) -> SignedBlob {
    let body = serde_json::to_vec(grant).unwrap_or_default();
    SignedBlob::signed(&grant.team, "keys", grant.roster_seq + 1, &body, root)
}

/// Read a `keys` slot blob: signed by the root, for this team.
pub fn read_grant(blob: &SignedBlob, team: &str, root: &str) -> Option<KeyGrant> {
    if blob.team != team || blob.slot != "keys" || !blob.verify(root) {
        return None;
    }
    let grant: KeyGrant = serde_json::from_slice(&blob.body_bytes()?).ok()?;
    (grant.team == team && blob.version == grant.roster_seq + 1).then_some(grant)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RosterBox {
    epoch: u64,
    nonce: String,
    ct: String,
}

fn roster_header(team: &str, version: u64, epoch: u64) -> Vec<u8> {
    format!("pm-roster-v1\n{team}\n{version}\n{epoch}").into_bytes()
}

/// The `roster` slot blob: the signed log, encrypted with the current epoch key, signed by the root.
pub fn roster_blob(roster: &Roster, key: &[u8; 32], root: &SigningKey) -> Result<SignedBlob, String> {
    let version = roster.seq() + 1;
    let plaintext = serde_json::to_vec(&roster.ops).map_err(|e| e.to_string())?;
    let (nonce, ct) = crypto::seal_unpadded(key, &roster_header(&roster.team, version, roster.epoch), &plaintext).map_err(|e| e.to_string())?;
    let body = serde_json::to_vec(&RosterBox { epoch: roster.epoch, nonce: b64(&nonce), ct: b64(&ct) }).map_err(|e| e.to_string())?;
    Ok(SignedBlob::signed(&roster.team, "roster", version, &body, root))
}

/// Open a `roster` slot blob with my keys. The caller verifies the log (`Roster::verify` /
/// `update_from`).
pub fn read_roster_blob(blob: &SignedBlob, team: &str, root: &str, keys: &KeyStore) -> Option<Vec<SignedOp>> {
    if blob.team != team || blob.slot != "roster" || !blob.verify(root) {
        return None;
    }
    let rbox: RosterBox = serde_json::from_slice(&blob.body_bytes()?).ok()?;
    let key = keys.epoch_key(team, rbox.epoch).ok().flatten()?;
    let plaintext = crypto::open_unpadded(&key, &unb64(&rbox.nonce)?, &roster_header(team, blob.version, rbox.epoch), &unb64(&rbox.ct)?).ok()?;
    serde_json::from_slice(&plaintext).ok()
}
