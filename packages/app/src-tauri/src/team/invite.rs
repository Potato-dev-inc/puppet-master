//! Invites (COWORK-PLAN §4.2): a link plus an 8-character code, shown with a QR code like phone
//! pairing (same look, never the pairing token: team members are not paired devices).
//!
//! - The **link** carries the team id, its root key and salt (so the joiner can check every
//!   roster operation), the transport address, the admin device's HPKE key and the invite id. It
//!   cannot join on its own.
//! - The **code** is the secret (single use, 10 minutes). The join request carries an HMAC with
//!   it, sealed to the admin's device, so neither the relay nor anyone who sees the link learns it.
//! - The **admin approves** every request by hand ("Let Leo in"); approval adds the member to the
//!   signed roster and seals the current team key to the new device.

use super::crypto::{self, b64, hmac, hpke_open, hpke_seal, same_mac, unb64, KemSecret};
use super::roster::DeviceEntry;
use pm_relay_protocol::{base32, team_id, unb64_32, JoinBox};
use serde::{Deserialize, Serialize};

/// Code length, as with phone pairing (`PAIRING_CODE_LEN`).
pub const CODE_LEN: usize = 8;
/// How long an invite works.
pub const INVITE_TTL_MS: u64 = 10 * 60 * 1000;
/// Wrong codes allowed per invite before it is closed.
pub const MAX_ATTEMPTS: u32 = 5;
/// No 0/O, 1/I/L, U, so a code read aloud cannot be mistyped.
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTVWXYZ";

/// An invite I made, as kept in my state (the code is in the credential store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: String,
    pub created_ms: u64,
    pub expires_ms: u64,
    #[serde(default)]
    pub used: bool,
    #[serde(default)]
    pub attempts: u32,
    /// A request with the right code arrived. Another one from someone else after that means the
    /// code leaked, and both are flagged (security review L8).
    #[serde(default)]
    pub proved: bool,
}

impl Invite {
    pub fn is_open(&self, now_ms: u64) -> bool {
        !self.used && self.expires_ms > now_ms && self.attempts < MAX_ATTEMPTS
    }
}

pub fn new_invite(now_ms: u64) -> (Invite, String) {
    let id = base32(&crypto::random_bytes::<10>());
    let code: String = crypto::random_bytes::<CODE_LEN>()
        .iter()
        .map(|b| CODE_ALPHABET[*b as usize % CODE_ALPHABET.len()] as char)
        .collect();
    (Invite { id, created_ms: now_ms, expires_ms: now_ms + INVITE_TTL_MS, used: false, attempts: 0, proved: false }, code)
}

/// `ABCD-EFGH` for display.
pub fn show_code(code: &str) -> String {
    if code.len() == CODE_LEN {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_string()
    }
}

/// What a person typed, as a code: upper case, without spaces or dashes.
pub fn normalize_code(typed: &str) -> String {
    typed.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// The invite link. `pm://team/join?…`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteLink {
    pub team: String,
    pub name: String,
    /// The team's root key and salt: the team id must match them.
    pub root: String,
    pub salt: String,
    /// Relay address, or a shared folder path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The admin device's HPKE key: the join request is sealed to it.
    pub admin_kem: String,
    pub invite: String,
}

fn enc(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn dec(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

impl InviteLink {
    pub fn to_url(&self) -> String {
        let mut url = format!(
            "pm://team/join?v=1&t={}&n={}&a={}&s={}&k={}&i={}",
            enc(&self.team),
            enc(&self.name),
            enc(&self.root),
            enc(&self.salt),
            enc(&self.admin_kem),
            enc(&self.invite)
        );
        if let Some(relay) = &self.relay {
            url.push_str(&format!("&r={}", enc(relay)));
        }
        if let Some(folder) = &self.folder {
            url.push_str(&format!("&f={}", enc(folder)));
        }
        url
    }

    /// Parse and check a pasted link (untrusted text).
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let query = text
            .strip_prefix("pm://team/join?")
            .ok_or("That isn't a team invite link. It starts with pm://team/join.")?;
        let mut get = std::collections::HashMap::new();
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            get.insert(k.to_string(), dec(v).ok_or("The invite link is damaged.")?);
        }
        let field = |k: &str| get.get(k).cloned().filter(|v| !v.is_empty());
        if field("v").as_deref() != Some("1") {
            return Err("This invite is from a newer Puppet Master. Update first.".into());
        }
        let link = InviteLink {
            team: field("t").ok_or("The invite link is incomplete.")?,
            name: super::event::clean_text(&field("n").unwrap_or_default(), 60),
            root: field("a").ok_or("The invite link is incomplete.")?,
            salt: field("s").ok_or("The invite link is incomplete.")?,
            relay: field("r"),
            folder: field("f"),
            admin_kem: field("k").ok_or("The invite link is incomplete.")?,
            invite: field("i").ok_or("The invite link is incomplete.")?,
        };
        let root = unb64_32(&link.root).ok_or("The invite link is damaged.")?;
        let salt = unb64(&link.salt).ok_or("The invite link is damaged.")?;
        if team_id(&root, &salt) != link.team {
            return Err("The invite link doesn't add up (its team id and key don't match).".into());
        }
        if unb64_32(&link.admin_kem).is_none() || link.invite.len() > 32 {
            return Err("The invite link is damaged.".into());
        }
        if link.relay.is_none() == link.folder.is_none() {
            return Err("The invite link names no way to reach the team.".into());
        }
        if link.folder.as_deref().is_some_and(is_network_path) {
            // Joining writes into that folder; on Windows a \\host\share path makes the computer
            // sign in to that host (an NTLM handshake) for whoever wrote the link.
            return Err("This invite points at a network folder. Ask for a relay link, or pick the shared folder yourself in Settings.".into());
        }
        Ok(link)
    }
}

/// A UNC or device path (`\\host\share`, `//host/share`, `\\?\…`, `\\.\…`, `\??\…`): not allowed in
/// a folder that comes from a link (security review L2). Typed in Settings, a share is fine.
pub fn is_network_path(folder: &str) -> bool {
    let f = folder.trim();
    let starts = |p: &str| f.len() >= p.len() && f.as_bytes()[..p.len()].iter().zip(p.bytes()).all(|(a, b)| (*a == b'/' && b == b'\\') || *a == b);
    starts(r"\\") || starts(r"\??\")
}

/// What a joiner sends (sealed to the admin's device).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    pub invite: String,
    /// The joiner's member key.
    pub member: String,
    pub device: DeviceEntry,
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    /// HMAC-SHA256 with the code over [`proof_message`], base64url.
    pub proof: String,
    pub at_ms: u64,
}

fn proof_message(team: &str, req: &JoinRequest) -> Vec<u8> {
    format!("pm-join-v1\n{team}\n{}\n{}\n{}\n{}", req.invite, req.member, req.device.id, req.device.kem).into_bytes()
}

fn join_info(team: &str) -> Vec<u8> {
    format!("pm-join-v1\n{team}").into_bytes()
}

/// Build and seal a join request for `link`, proving knowledge of `code`.
pub fn seal_join(link: &InviteLink, code: &str, mut req: JoinRequest) -> Result<JoinBox, String> {
    req.invite = link.invite.clone();
    req.proof = b64(&hmac(normalize_code(code).as_bytes(), &proof_message(&link.team, &req)));
    let plaintext = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
    let (enc, ct) = hpke_seal(&link.admin_kem, &join_info(&link.team), &plaintext).map_err(|e| e.to_string())?;
    Ok(JoinBox { team: link.team.clone(), invite: link.invite.clone(), enc: b64(&enc), ct: b64(&ct), at_ms: req.at_ms })
}

/// Open a join request sealed to my (admin) device. `None` when it isn't for me or is damaged.
pub fn open_join(kem: &KemSecret, join: &JoinBox) -> Option<JoinRequest> {
    let plain = hpke_open(kem, &unb64(&join.enc)?, &join_info(&join.team), &unb64(&join.ct)?).ok()?;
    let req: JoinRequest = serde_json::from_slice(&plain).ok()?;
    (req.invite == join.invite).then_some(req)
}

/// The code proof is right, the device is certified by the joining member, the name is sane.
pub fn check_join(team: &str, req: &JoinRequest, code: &str) -> Result<(), &'static str> {
    let Some(proof) = unb64(&req.proof) else { return Err("damaged request") };
    if !same_mac(&proof, &hmac(normalize_code(code).as_bytes(), &proof_message(team, req))) {
        return Err("wrong code");
    }
    let cert = super::roster::device_cert_bytes(team, &req.member, &req.device.id, &req.device.kem);
    if !crypto::verify_sig(&req.member, &cert, &req.device.cert) {
        return Err("the device is not certified by its member key");
    }
    if super::event::clean_text(&req.display, 40).is_empty() {
        return Err("no name");
    }
    if let Some(login) = &req.github {
        if login.is_empty() || login.len() > 39 || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("bad GitHub login");
        }
    }
    Ok(())
}
