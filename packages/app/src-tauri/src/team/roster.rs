//! The signed roster (COWORK-PLAN §4.1): who is on the team, with which devices and roles.
//!
//! It is an append-only log of operations. Each operation names the one before it by hash and is
//! signed by the team's root key, which is the admin's member key (v1 has one admin: whoever
//! created the team). The team id is a hash of that root key and a salt
//! (`pm_relay_protocol::team_id`), so a teammate who knows the team id can check the whole chain
//! from the first operation, and nobody without the root key (a relay, a folder, another member)
//! can add a member, a device or a role.
//!
//! Devices are certified by their member's own key, so the admin cannot invent a device for
//! someone either.

use super::crypto::{key_id, sign, verify_sig, SigningKey};
use pm_relay_protocol::{hex, is_key_id, sha256, team_id, unb64, unb64_32};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Receive events, see presence and claims. Every member.
    Watch,
    /// Answer my forwarded questions (v1.1).
    Approve,
    /// Roster, roles, keys. v1: the creator only.
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    /// base64url Ed25519 public key: signs envelopes and relay requests.
    pub id: String,
    /// base64url X25519 public key: receives sealed keys.
    pub kem: String,
    /// "Maya's desktop".
    #[serde(default)]
    pub name: String,
    /// The member key's signature over [`device_cert_bytes`].
    pub cert: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberEntry {
    /// base64url Ed25519 public key (the member id).
    pub id: String,
    pub display: String,
    /// GitHub login the member claimed when joining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    /// The admin confirmed the login.
    #[serde(default)]
    pub github_confirmed: bool,
    pub roles: Vec<Role>,
    pub devices: Vec<DeviceEntry>,
}

impl MemberEntry {
    pub fn has(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpBody {
    /// The first operation: the team's name, the salt of its id and its admin.
    Genesis { name: String, salt: String, admin: MemberEntry },
    Add { member: MemberEntry },
    AddDevice { member: String, device: DeviceEntry },
    Remove { member: String },
    RemoveDevice { member: String, device: String },
    SetRoles { member: String, roles: Vec<Role> },
    ConfirmGithub { member: String },
    /// A new team key (epoch). Written on every removal.
    Rotate { epoch: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterOp {
    pub team: String,
    /// 0 for genesis, then +1.
    pub seq: u64,
    /// Hash of the previous signed operation (`""` for genesis).
    pub prev: String,
    pub at_ms: u64,
    pub op: OpBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOp {
    pub body: RosterOp,
    /// Root key signature over the canonical JSON of `body`.
    pub sig: String,
}

impl SignedOp {
    fn signing_bytes(body: &RosterOp) -> Vec<u8> {
        let mut bytes = b"pm-roster-op-v1\n".to_vec();
        bytes.extend(serde_json::to_vec(body).unwrap_or_default());
        bytes
    }

    pub fn hash(&self) -> String {
        hex(&sha256(&[&serde_json::to_vec(self).unwrap_or_default()]))
    }
}

/// What a member signs to vouch for one of their devices.
pub fn device_cert_bytes(team: &str, member: &str, device: &str, kem: &str) -> Vec<u8> {
    format!("pm-device-cert-v1\n{team}\n{member}\n{device}\n{kem}").into_bytes()
}

/// A device entry for `device_key` / `kem_public`, certified by `member_key`.
pub fn certify_device(team: &str, member_key: &SigningKey, device_key: &SigningKey, kem_public: &str, name: &str) -> DeviceEntry {
    let member = key_id(member_key);
    let id = key_id(device_key);
    let cert = sign(member_key, &device_cert_bytes(team, &member, &id, kem_public));
    DeviceEntry { id, kem: kem_public.to_string(), name: super::event::clean_text(name, 40), cert }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterError(pub String);

impl std::fmt::Display for RosterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<T>(why: impl Into<String>) -> Result<T, RosterError> {
    Err(RosterError(why.into()))
}

/// The roster after checking every operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Roster {
    pub team: String,
    pub name: String,
    /// The root (admin) key, base64url.
    pub root: String,
    pub salt: String,
    pub members: Vec<MemberEntry>,
    /// Devices that were removed (never accepted again).
    pub removed_devices: Vec<String>,
    pub removed_members: Vec<String>,
    /// The current team key epoch (1 at creation).
    pub epoch: u64,
    pub ops: Vec<SignedOp>,
}

fn check_member(team: &str, member: &MemberEntry) -> Result<(), RosterError> {
    if !is_key_id(&member.id) {
        return err("a member key is malformed");
    }
    if member.display.trim().is_empty() || member.display.chars().count() > 40 {
        return err("a member name is missing or too long");
    }
    for device in &member.devices {
        check_device(team, &member.id, device)?;
    }
    Ok(())
}

fn check_device(team: &str, member: &str, device: &DeviceEntry) -> Result<(), RosterError> {
    if !is_key_id(&device.id) || unb64_32(&device.kem).is_none() {
        return err("a device key is malformed");
    }
    if !verify_sig(member, &device_cert_bytes(team, member, &device.id, &device.kem), &device.cert) {
        return err("a device is not certified by its member");
    }
    Ok(())
}

impl Roster {
    /// Check a whole log from genesis. `expected_team` pins the team id (from the invite link or
    /// my own config), which pins the root key.
    pub fn verify(expected_team: &str, ops: &[SignedOp]) -> Result<Roster, RosterError> {
        let Some(first) = ops.first() else { return err("the roster is empty") };
        let OpBody::Genesis { name, salt, admin } = &first.body.op else {
            return err("the roster does not start with its genesis");
        };
        let (Some(root_bytes), Some(salt_bytes)) = (unb64_32(&admin.id), unb64(salt)) else {
            return err("the genesis is malformed");
        };
        if team_id(&root_bytes, &salt_bytes) != expected_team || first.body.team != expected_team {
            return err("the roster belongs to another team");
        }
        let mut roster = Roster {
            team: expected_team.to_string(),
            name: name.clone(),
            root: admin.id.clone(),
            salt: salt.clone(),
            members: Vec::new(),
            removed_devices: Vec::new(),
            removed_members: Vec::new(),
            epoch: 1,
            ops: Vec::new(),
        };
        for (i, op) in ops.iter().enumerate() {
            roster.apply(op, i == 0)?;
        }
        Ok(roster)
    }

    fn apply(&mut self, signed: &SignedOp, genesis: bool) -> Result<(), RosterError> {
        let body = &signed.body;
        let expected_prev = self.ops.last().map(SignedOp::hash).unwrap_or_default();
        if body.team != self.team || body.seq != self.ops.len() as u64 || body.prev != expected_prev {
            return err(format!("roster operation {} is out of order", body.seq));
        }
        if !verify_sig(&self.root, &SignedOp::signing_bytes(body), &signed.sig) {
            return err(format!("roster operation {} is not signed by the team's admin", body.seq));
        }
        match (&body.op, genesis) {
            (OpBody::Genesis { admin, .. }, true) => {
                check_member(&self.team, admin)?;
                if !admin.has(Role::Admin) || admin.devices.is_empty() {
                    return err("the genesis admin is incomplete");
                }
                self.members.push(admin.clone());
            }
            (OpBody::Genesis { .. }, false) => return err("a second genesis"),
            (_, true) => return err("the roster does not start with its genesis"),
            (OpBody::Add { member }, _) => {
                check_member(&self.team, member)?;
                if member.has(Role::Admin) {
                    return err("v1 has one admin");
                }
                if self.member(&member.id).is_some() || self.removed_members.contains(&member.id) {
                    return err("that member is already known");
                }
                for d in &member.devices {
                    if self.device(&d.id).is_some() || self.removed_devices.contains(&d.id) {
                        return err("that device is already known");
                    }
                }
                self.members.push(member.clone());
            }
            (OpBody::AddDevice { member, device }, _) => {
                check_device(&self.team, member, device)?;
                if self.device(&device.id).is_some() || self.removed_devices.contains(&device.id) {
                    return err("that device is already known");
                }
                let Some(m) = self.members.iter_mut().find(|m| &m.id == member) else { return err("no such member") };
                m.devices.push(device.clone());
            }
            (OpBody::Remove { member }, _) => {
                if member == &self.root {
                    return err("the admin cannot be removed");
                }
                let Some(at) = self.members.iter().position(|m| &m.id == member) else { return err("no such member") };
                let gone = self.members.remove(at);
                self.removed_members.push(gone.id);
                self.removed_devices.extend(gone.devices.into_iter().map(|d| d.id));
            }
            (OpBody::RemoveDevice { member, device }, _) => {
                let Some(m) = self.members.iter_mut().find(|m| &m.id == member) else { return err("no such member") };
                let Some(at) = m.devices.iter().position(|d| &d.id == device) else { return err("no such device") };
                m.devices.remove(at);
                self.removed_devices.push(device.clone());
            }
            (OpBody::SetRoles { member, roles }, _) => {
                if roles.contains(&Role::Admin) != (member == &self.root) {
                    return err("v1 has one admin");
                }
                let Some(m) = self.members.iter_mut().find(|m| &m.id == member) else { return err("no such member") };
                let mut roles = roles.clone();
                if !roles.contains(&Role::Watch) {
                    roles.insert(0, Role::Watch);
                }
                roles.sort();
                roles.dedup();
                m.roles = roles;
            }
            (OpBody::ConfirmGithub { member }, _) => {
                let Some(m) = self.members.iter_mut().find(|m| &m.id == member) else { return err("no such member") };
                m.github_confirmed = m.github.is_some();
            }
            (OpBody::Rotate { epoch }, _) => {
                if *epoch != self.epoch + 1 {
                    return err("team keys must rotate in order");
                }
                self.epoch = *epoch;
            }
        }
        self.ops.push(signed.clone());
        Ok(())
    }

    /// Start a team: I am its admin, with this device.
    pub fn create(name: &str, root: &SigningKey, display: &str, device: impl FnOnce(&str) -> DeviceEntry, now_ms: u64) -> Result<Roster, RosterError> {
        let salt = pm_relay_protocol::b64(&super::crypto::random_bytes::<16>());
        let team = team_id(root.verifying_key().as_bytes(), &unb64(&salt).unwrap_or_default());
        let admin = MemberEntry {
            id: key_id(root),
            display: super::event::clean_text(display, 40),
            github: None,
            github_confirmed: false,
            roles: vec![Role::Watch, Role::Approve, Role::Admin],
            devices: vec![device(&team)],
        };
        let body = RosterOp {
            team: team.clone(),
            seq: 0,
            prev: String::new(),
            at_ms: now_ms,
            op: OpBody::Genesis { name: super::event::clean_text(name, 60), salt, admin },
        };
        let sig = sign(root, &SignedOp::signing_bytes(&body));
        Roster::verify(&team, &[SignedOp { body, sig }])
    }

    /// Append one operation as the admin. The roster is unchanged when it is refused.
    pub fn append(&mut self, op: OpBody, root: &SigningKey, now_ms: u64) -> Result<SignedOp, RosterError> {
        if key_id(root) != self.root {
            return err("only the team's admin can change the roster");
        }
        let body = RosterOp {
            team: self.team.clone(),
            seq: self.ops.len() as u64,
            prev: self.ops.last().map(SignedOp::hash).unwrap_or_default(),
            at_ms: now_ms,
            op,
        };
        let signed = SignedOp { sig: sign(root, &SignedOp::signing_bytes(&body)), body };
        let mut next = self.clone();
        next.apply(&signed, false)?;
        *self = next;
        Ok(signed)
    }

    /// Accept a newer copy of the log only if it extends mine (never a rollback or a fork).
    pub fn update_from(&mut self, ops: &[SignedOp]) -> Result<bool, RosterError> {
        if ops.len() < self.ops.len() || ops[..self.ops.len()] != self.ops[..] {
            return err("the team list from the relay does not extend mine");
        }
        if ops.len() == self.ops.len() {
            return Ok(false);
        }
        *self = Roster::verify(&self.team, ops)?;
        Ok(true)
    }

    pub fn seq(&self) -> u64 {
        self.ops.len().saturating_sub(1) as u64
    }

    pub fn member(&self, id: &str) -> Option<&MemberEntry> {
        self.members.iter().find(|m| m.id == id)
    }

    /// The member and entry of an active device.
    pub fn device(&self, id: &str) -> Option<(&MemberEntry, &DeviceEntry)> {
        self.members.iter().find_map(|m| m.devices.iter().find(|d| d.id == id).map(|d| (m, d)))
    }

    pub fn all_devices(&self) -> Vec<&DeviceEntry> {
        self.members.iter().flat_map(|m| m.devices.iter()).collect()
    }

    pub fn device_ids(&self) -> Vec<String> {
        self.all_devices().into_iter().map(|d| d.id.clone()).collect()
    }

    pub fn is_admin(&self, member: &str) -> bool {
        member == self.root
    }

    /// The member whose GitHub login this is (for GitHub facts).
    pub fn by_github(&self, login: &str) -> Option<&MemberEntry> {
        self.members.iter().find(|m| m.github.as_deref().is_some_and(|g| g.eq_ignore_ascii_case(login)))
    }
}
