//! Creating a team, letting people in, removing them, and keeping my copy of the roster and keys
//! current. These run over any transport (relay or folder) and are tested with both.
//!
//! The admin's app publishes three things after every roster change, all signed by the root key:
//! 1. the `keys` slot: the current team key sealed (HPKE) to every active device;
//! 2. the `roster` slot: the signed roster log, encrypted with the current team key;
//! 3. (relay) the access list: which device keys may use the mailbox.
//!
//! Removing a member always rotates the team key, so the removed devices can't read anything
//! sent afterwards (they keep what they already received, like any chat).

use super::crypto::{self, key_id};
use super::invite::{self, InviteLink, JoinRequest};
use super::keystore::KeyStore;
use super::roster::{certify_device, MemberEntry, OpBody, Role, Roster, SignedOp};
use super::secure::{grant_blob, make_grant, open_grant, read_grant, read_roster_blob, roster_blob, Payload, Session};
use super::transport::TeamTransport;
use pm_relay_protocol::{SignedAcl, TeamRegistration};

/// The device name shown in the roster ("Maya's computer").
pub fn device_name(display: &str) -> String {
    let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
    let profile = crate::app_paths::profile();
    match (host.trim(), profile) {
        ("", None) => format!("{display}'s computer"),
        (host, None) => host.to_string(),
        ("", Some(p)) => format!("{display}'s computer ({p})"),
        (host, Some(p)) => format!("{host} ({p})"),
    }
}

/// Publish the roster, the key grant and the access list (admin only).
pub fn publish(roster: &Roster, keys: &KeyStore, transport: &dyn TeamTransport) -> Result<(), String> {
    let root = keys.member()?;
    if key_id(&root) != roster.root {
        return Err("Only the team's admin can change who is on it.".into());
    }
    let key = keys.epoch_key(&roster.team, roster.epoch)?.ok_or("The current team key is missing on this computer.")?;
    let acl = SignedAcl::signed(&roster.team, roster.seq() + 1, roster.device_ids(), &root);
    // Access first, so a newly let-in device can read the slots as soon as they are there.
    transport.set_acl(&acl).map_err(|e| e.0)?;
    transport.put_state(&grant_blob(&make_grant(roster, &key)?, &root)).map_err(|e| e.0)?;
    transport.put_state(&roster_blob(roster, &key, &root)?).map_err(|e| e.0)?;
    Ok(())
}

/// Tell the team's apps to fetch the new roster and keys (sealed with the current key).
pub fn announce(session: &Session, seq_out: u64, transport: &dyn TeamTransport, now_ms: u64) -> Result<(), String> {
    let env = session.seal(&Payload::RosterChanged { seq: session.roster.seq() }, seq_out, now_ms)?;
    transport.send(&env).map_err(|e| e.0)
}

/// Start a team with me as its admin. The team id is known only after the genesis, so the
/// transport is made for it then (`transport_for(team_id)`).
pub fn create_team(
    name: &str,
    display: &str,
    keys: &KeyStore,
    transport_for: impl FnOnce(&str) -> Result<std::sync::Arc<dyn TeamTransport>, String>,
    now_ms: u64,
) -> Result<Roster, String> {
    let root = keys.member()?;
    let device = keys.device_sign()?;
    let kem = keys.device_kem()?;
    let roster = Roster::create(name, &root, display, |team| certify_device(team, &root, &device, &kem.public_b64(), &device_name(display)), now_ms)
        .map_err(|e| e.0)?;
    let transport = transport_for(&roster.team)?;
    keys.set_epoch_key(&roster.team, 1, &crypto::random_bytes::<32>())?;
    let registration = TeamRegistration {
        team: roster.team.clone(),
        root: roster.root.clone(),
        salt: roster.salt.clone(),
        acl: SignedAcl::signed(&roster.team, roster.seq() + 1, roster.device_ids(), &root),
    };
    transport.register(&registration).map_err(|e| e.0)?;
    publish(&roster, keys, transport.as_ref())?;
    Ok(roster)
}

/// The link for an invite I made.
pub fn invite_link(roster: &Roster, keys: &KeyStore, relay: Option<&str>, folder: Option<&str>, invite_id: &str) -> Result<InviteLink, String> {
    Ok(InviteLink {
        team: roster.team.clone(),
        name: roster.name.clone(),
        root: roster.root.clone(),
        salt: roster.salt.clone(),
        relay: relay.filter(|r| !r.is_empty()).map(str::to_string),
        folder: folder.filter(|f| !f.is_empty()).map(str::to_string),
        admin_kem: keys.device_kem()?.public_b64(),
        invite: invite_id.to_string(),
    })
}

/// Send my join request for `link` (the joiner's side).
pub fn send_join(
    link: &InviteLink,
    code: &str,
    display: &str,
    github: Option<String>,
    keys: &KeyStore,
    transport: &dyn TeamTransport,
    now_ms: u64,
) -> Result<(), String> {
    let member = keys.member()?;
    let device = keys.device_sign()?;
    let kem = keys.device_kem()?;
    let request = JoinRequest {
        invite: link.invite.clone(),
        member: key_id(&member),
        device: certify_device(&link.team, &member, &device, &kem.public_b64(), &device_name(display)),
        display: super::event::clean_text(display, 40),
        github: github.map(|g| g.trim().to_string()).filter(|g| !g.is_empty()),
        proof: String::new(),
        at_ms: now_ms,
    };
    let sealed = invite::seal_join(link, code, request)?;
    transport.post_join(&sealed).map_err(|e| e.0)
}

/// Joiner: am I in yet? `Ok(Some(roster))` once the admin let me in and the key reached me.
pub fn try_finish_join(link: &InviteLink, keys: &KeyStore, transport: &dyn TeamTransport) -> Result<Option<Roster>, String> {
    let blob = match transport.get_state("keys") {
        Ok(Some(blob)) => blob,
        Ok(None) | Err(_) => return Ok(None), // not let in yet (the relay refuses until then)
    };
    let Some(grant) = read_grant(&blob, &link.team, &link.root) else { return Ok(None) };
    let device = key_id(&keys.device_sign()?);
    let Some(key) = open_grant(&grant, &device, &keys.device_kem()?) else { return Ok(None) };
    keys.set_epoch_key(&link.team, grant.epoch, &key)?;
    let Some(roster_blob) = transport.get_state("roster").map_err(|e| e.0)? else { return Ok(None) };
    let Some(ops) = read_roster_blob(&roster_blob, &link.team, &link.root, keys) else { return Ok(None) };
    let roster = Roster::verify(&link.team, &ops).map_err(|e| e.0)?;
    if roster.device(&device).is_none() {
        return Ok(None);
    }
    Ok(Some(roster))
}

/// What a refresh found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Refreshed {
    /// The roster has new operations.
    pub roster_changed: bool,
    /// A new team key reached me.
    pub new_epoch: bool,
    /// My device is no longer on the team.
    pub removed: bool,
}

/// Member: fetch the latest key grant and roster and accept them if they extend mine.
pub fn refresh(roster: &mut Roster, keys: &KeyStore, transport: &dyn TeamTransport) -> Result<Refreshed, String> {
    let mut out = Refreshed::default();
    let device = key_id(&keys.device_sign()?);
    let keys_slot = match transport.get_state("keys") {
        Ok(slot) => slot,
        // The relay's access list (root-signed) no longer has this device: removed.
        Err(e) if e.is_denied() => {
            out.removed = true;
            return Ok(out);
        }
        Err(e) => return Err(e.0),
    };
    if let Some(blob) = keys_slot {
        if let Some(grant) = read_grant(&blob, &roster.team, &roster.root) {
            if keys.epoch_key(&roster.team, grant.epoch)?.is_none() {
                if let Some(key) = open_grant(&grant, &device, &keys.device_kem()?) {
                    keys.set_epoch_key(&roster.team, grant.epoch, &key)?;
                    out.new_epoch = true;
                }
            }
        }
    }
    if let Some(blob) = transport.get_state("roster").map_err(|e| e.0)? {
        if let Some(ops) = read_roster_blob(&blob, &roster.team, &roster.root, keys) {
            out.roster_changed = roster.update_from(&ops).map_err(|e| e.0)?;
        }
    }
    out.removed = roster.device(&device).is_none();
    Ok(out)
}

/// Admin: let a checked join request in, as a watching member.
pub fn approve(roster: &mut Roster, request: &JoinRequest, keys: &KeyStore, transport: &dyn TeamTransport, now_ms: u64) -> Result<SignedOp, String> {
    let root = keys.member()?;
    let member = MemberEntry {
        id: request.member.clone(),
        display: super::event::clean_text(&request.display, 40),
        github: request.github.clone(),
        github_confirmed: false,
        roles: vec![Role::Watch],
        devices: vec![request.device.clone()],
    };
    let mut next = roster.clone();
    let op = next.append(OpBody::Add { member }, &root, now_ms).map_err(|e| e.0)?;
    publish(&next, keys, transport)?;
    *roster = next;
    Ok(op)
}

/// Admin: remove a member and rotate the team key.
pub fn remove(roster: &mut Roster, member: &str, keys: &KeyStore, transport: &dyn TeamTransport, now_ms: u64) -> Result<u64, String> {
    let root = keys.member()?;
    let mut next = roster.clone();
    next.append(OpBody::Remove { member: member.to_string() }, &root, now_ms).map_err(|e| e.0)?;
    let epoch = next.epoch + 1;
    next.append(OpBody::Rotate { epoch }, &root, now_ms).map_err(|e| e.0)?;
    keys.set_epoch_key(&next.team, epoch, &crypto::random_bytes::<32>())?;
    publish(&next, keys, transport)?;
    *roster = next;
    Ok(epoch)
}

/// Rotate the team key every this often, even without a removal (COWORK-PLAN §8.2).
pub const ROTATE_EVERY_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// When the current team key was made: the last rotation, or the team's creation.
pub fn key_made_at(roster: &Roster) -> u64 {
    roster
        .ops
        .iter()
        .rev()
        .find(|op| matches!(op.body.op, OpBody::Rotate { .. } | OpBody::Genesis { .. }))
        .map(|op| op.body.at_ms)
        .unwrap_or(0)
}

/// Admin: a new team key for everyone still on the team.
pub fn rotate(roster: &mut Roster, keys: &KeyStore, transport: &dyn TeamTransport, now_ms: u64) -> Result<u64, String> {
    let root = keys.member()?;
    let mut next = roster.clone();
    let epoch = next.epoch + 1;
    next.append(OpBody::Rotate { epoch }, &root, now_ms).map_err(|e| e.0)?;
    keys.set_epoch_key(&next.team, epoch, &crypto::random_bytes::<32>())?;
    publish(&next, keys, transport)?;
    *roster = next;
    Ok(epoch)
}
