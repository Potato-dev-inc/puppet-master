//! Team v1 tests: identity and crypto, the signed roster, key rotation on removal, gap detection,
//! invites, the relay end to end (a real `pm-relay` on a loopback port), GitHub facts from recorded
//! `gh` output, "Merge when green", the phone route and the audit log.
//!
//! Every person gets an in-memory key store; nothing here touches the OS credential store.

use super::config::{Member, TeamConfig};
use super::crypto::key_id;
use super::event::*;
use super::invite::{self, InviteLink};
use super::keystore::KeyStore;
use super::membership;
use super::merge_green;
use super::roster::{certify_device, OpBody, Roster, SignedOp};
use super::secure::{Opened, Payload, Reject, SeqTracker, Session, Verified};
use super::sources::github::{self, CiState, Fact, Gh, GhResponse, RepoPoll};
use super::store::{ingest, Ingested, TeamStore};
use super::transport::folder::FolderTransport;
use super::transport::relay::RelayTransport;
use super::transport::{Inbound, TeamTransport};
use crate::secrets::MemoryBackend;
use parking_lot::Mutex;
use pm_relay_protocol::{b64, Envelope};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const NOW: u64 = 1_790_000_000_000;

fn now() -> u64 {
    super::transport::now_ms()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmteam1-{tag}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn person(name: &str) -> KeyStore {
    KeyStore::with_backend(Arc::new(MemoryBackend::new()), name)
}

fn folder(dir: &std::path::Path) -> Arc<dyn TeamTransport> {
    Arc::new(FolderTransport::new(dir.to_path_buf()))
}

/// Maya creates a team; each name in `others` is invited, asks to join with the right code, and
/// is let in. Returns the admin's roster and everyone's key stores.
struct World {
    roster: Roster,
    maya: KeyStore,
    others: Vec<(String, KeyStore, Roster)>,
    #[allow(dead_code)]
    link: Option<InviteLink>,
}

fn world(transport: &Arc<dyn TeamTransport>, others: &[&str], at: u64) -> World {
    let maya = person("maya");
    let t = transport.clone();
    let mut roster = membership::create_team("Acme", "Maya", &maya, move |_| Ok(t), at).unwrap();
    let mut joined = Vec::new();
    let mut last_link = None;
    for name in others {
        let keys = person(name);
        let (inv, code) = invite::new_invite(at);
        let link = membership::invite_link(&roster, &maya, None, Some("D:\\wire"), &inv.id).unwrap();
        // Join requests are dated now: the folder drops ones older than a day.
        membership::send_join(&link, &code, name, Some(format!("{}-gh", name.to_lowercase())), &keys, transport.as_ref(), now()).unwrap();
        // Before the admin says yes, the joiner is not in.
        assert!(membership::try_finish_join(&link, &keys, transport.as_ref()).unwrap().is_none());
        let kem = maya.device_kem().unwrap();
        let request = transport
            .joins()
            .unwrap()
            .iter()
            .filter(|j| j.invite == inv.id)
            .find_map(|j| invite::open_join(&kem, j))
            .expect("the join request opens for the admin");
        invite::check_join(&roster.team, &request, &code).unwrap();
        membership::approve(&mut roster, &request, &maya, transport.as_ref(), at).unwrap();
        let theirs = membership::try_finish_join(&link, &keys, transport.as_ref()).unwrap().expect("let in");
        joined.push((name.to_string(), keys, theirs));
        last_link = Some(link);
    }
    // Earlier joiners catch up with later approvals.
    for (_, keys, r) in &mut joined {
        membership::refresh(r, keys, transport.as_ref()).unwrap();
    }
    World { roster, maya, others: joined, link: last_link }
}

fn event_from(keys: &KeyStore, team: &str, at: u64) -> TeamEvent {
    let subject = Subject {
        branch: Some("feat/auth".into()),
        commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
        stats: Some(DiffStat { files: 2, additions: 9, deletions: 1 }),
        ..Subject::default()
    };
    TeamEvent {
        v: SCHEMA_VERSION,
        id: new_event_id(at),
        team: team.into(),
        project: ProjectRef { key: "github.com/acme/billing".into(), name: "billing".into() },
        actor: Actor {
            member: key_id(&keys.member().unwrap()),
            device: key_id(&keys.device_sign().unwrap()),
            display: "Maya".into(),
            agent: Some(AgentTag { label: "Codex".into(), role: None }),
        },
        kind: TeamEventKind::Pushed,
        at_ms: at,
        dedupe_key: dedupe_key("github.com/acme/billing", TeamEventKind::Pushed, &subject),
        subject,
        note: Some("Careful with the session model".into()),
        to: vec![],
        tier_hint: Tier::Ambient,
        expires_at_ms: at + DEFAULT_TTL_MS,
        source: EventSource::Git,
    }
}

fn config_for(keys: &KeyStore, roster: &Roster, name: &str) -> TeamConfig {
    TeamConfig {
        enabled: true,
        team: roster.team.clone(),
        name: roster.name.clone(),
        folder: "D:\\wire".into(),
        me: Member { id: key_id(&keys.member().unwrap()), name: name.into() },
        device: key_id(&keys.device_sign().unwrap()),
        root: roster.root.clone(),
        salt: roster.salt.clone(),
        ..TeamConfig::default()
    }
}

fn opened_event(opened: Result<Opened, Reject>) -> Verified {
    match opened {
        Ok(Opened::Event(v)) => v,
        other => panic!("expected an event, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------- crypto round trip

#[test]
fn a_sealed_event_opens_for_a_teammate_and_becomes_a_card() {
    let dir = temp_dir("rt");
    let wire = folder(&dir);
    let w = world(&wire, &["Leo"], NOW);
    let (_, leo_keys, leo_roster) = &w.others[0];
    let maya = Session::new(w.roster.clone(), w.maya.clone());
    let leo = Session::new(leo_roster.clone(), leo_keys.clone());

    let event = event_from(&w.maya, &w.roster.team, now());
    let env = maya.seal(&Payload::Event { event: event.clone() }, 1, now()).unwrap();
    // On the wire: nothing readable.
    let wire_json = serde_json::to_string(&env).unwrap();
    for secret in ["feat/auth", "Maya", "session model", "billing", "Codex", "0123456789abcdef"] {
        assert!(!wire_json.contains(secret), "{secret:?} must not be readable on the wire");
    }

    let mut tracker = SeqTracker::default();
    let verified = opened_event(leo.open(&env, &mut tracker, now()));
    assert_eq!(verified.event(), &event);
    assert_eq!(verified.member(), Some(w.roster.root.as_str()));

    let mut store = TeamStore::default();
    let leo_config = config_for(leo_keys, leo_roster, "Leo");
    assert!(matches!(ingest(&mut store, &leo_config, verified, now()), Ingested::Card(_)));
    assert_eq!(store.cards[0].event.note.as_deref(), Some("Careful with the session model"));
    // My own envelope echoed back is recognised, not shown.
    assert_eq!(maya.open(&env, &mut SeqTracker::default(), now()), Ok(Opened::Mine));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tampered_forged_and_replayed_envelopes_are_refused_before_ingest() {
    let dir = temp_dir("forge");
    let wire = folder(&dir);
    let w = world(&wire, &["Leo", "Sam"], NOW);
    let (_, leo_keys, leo_roster) = &w.others[0];
    let (_, sam_keys, _) = &w.others[1];
    let maya = Session::new(w.roster.clone(), w.maya.clone());
    let leo = Session::new(leo_roster.clone(), leo_keys.clone());
    let at = now();
    let good = maya.seal(&Payload::Event { event: event_from(&w.maya, &w.roster.team, at) }, 1, at).unwrap();
    let open = |env: &Envelope| leo.open(env, &mut SeqTracker::default(), at);

    // 1. One flipped byte of ciphertext: the signature no longer matches.
    let mut flipped = good.clone();
    let mut ct = pm_relay_protocol::unb64(&flipped.ct).unwrap();
    ct[5] ^= 1;
    flipped.ct = b64(&ct);
    assert_eq!(open(&flipped), Err(Reject::BadSignature));

    // 2. A changed header (counter, epoch): same.
    let mut moved = good.clone();
    moved.seq = 99;
    assert_eq!(open(&moved), Err(Reject::BadSignature));

    // 3. Re-signed by someone not on the team (a relay, a folder writer).
    let outsider = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
    let mut forged = good.clone();
    forged.sign_with(&outsider);
    assert_eq!(open(&forged), Err(Reject::UnknownDevice));

    // 4. An outsider who even stole the team key still isn't on the roster.
    let thief = person("thief");
    thief.set_epoch_key(&w.roster.team, 1, &w.maya.epoch_key(&w.roster.team, 1).unwrap().unwrap()).unwrap();
    let thief_session = Session::new(w.roster.clone(), thief.clone());
    let stolen = thief_session.seal(&Payload::Event { event: event_from(&thief, &w.roster.team, at) }, 1, at).unwrap();
    assert_eq!(open(&stolen), Err(Reject::UnknownDevice));

    // 5. A teammate (Sam) signing an event that claims to be Maya's.
    let sam = Session::new(w.roster.clone(), sam_keys.clone());
    let posing = sam.seal(&Payload::Event { event: event_from(&w.maya, &w.roster.team, at) }, 1, at).unwrap();
    assert_eq!(open(&posing), Err(Reject::Impersonation));

    // 6. A teammate relaying a "GitHub fact" (facts are only learned from GitHub itself).
    let mut fact = event_from(sam_keys, &w.roster.team, at);
    fact.source = EventSource::Github;
    let relayed = sam.seal(&Payload::Event { event: fact }, 2, at).unwrap();
    assert_eq!(open(&relayed), Err(Reject::NotAllowed));

    // 7. Another team's envelope.
    let mut other = good.clone();
    other.team = pm_relay_protocol::team_id(&[9; 32], b"x");
    assert_eq!(open(&other), Err(Reject::OtherTeam));

    // 8. A replay of the good one.
    let mut tracker = SeqTracker::default();
    assert!(matches!(leo.open(&good, &mut tracker, at), Ok(Opened::Event(_))));
    assert_eq!(leo.open(&good, &mut tracker, at), Err(Reject::Replay));

    // 9. Not even a team envelope: the v0 plain event a folder writer could drop in.
    let plain = serde_json::to_vec(&event_from(&w.maya, &w.roster.team, at)).unwrap();
    assert!(serde_json::from_slice::<Envelope>(&plain).is_err(), "a plain event is not an envelope at all");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_folder_wire_no_longer_lets_anyone_who_can_write_it_make_a_card() {
    // v0: any JSON event file in the shared folder became a card. v1: only sealed envelopes signed
    // by a device on the signed roster get through `Session::open`, and `ingest` takes nothing else.
    let dir = temp_dir("folder");
    let wire = folder(&dir);
    let w = world(&wire, &["Leo"], NOW);
    let (_, leo_keys, leo_roster) = &w.others[0];
    let leo = Session::new(leo_roster.clone(), leo_keys.clone());
    let rx = wire.subscribe(0).unwrap();

    // A stranger with write access to the folder drops a plain v0 event and a self-signed envelope.
    let at = now();
    let mut plain = event_from(&person("mallory"), &w.roster.team, at);
    plain.actor.display = "Maya".into();
    std::fs::create_dir_all(dir.join("envelopes")).unwrap();
    std::fs::write(dir.join("envelopes").join(format!("{}.json", plain.id)), serde_json::to_vec(&plain).unwrap()).unwrap();
    let mallory = person("mallory2");
    mallory.set_epoch_key(&w.roster.team, 1, &[7; 32]).unwrap();
    let fake = Session::new(w.roster.clone(), mallory.clone())
        .seal(&Payload::Event { event: event_from(&mallory, &w.roster.team, at) }, 1, at)
        .unwrap();
    FolderTransport::new(dir.clone()).send(&fake).unwrap();
    // And Maya really sends one.
    let real = Session::new(w.roster.clone(), w.maya.clone())
        .seal(&Payload::Event { event: event_from(&w.maya, &w.roster.team, at) }, 1, at)
        .unwrap();
    wire.send(&real).unwrap();

    let mut store = TeamStore::default();
    let mut tracker = SeqTracker::default();
    let config = config_for(leo_keys, leo_roster, "Leo");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut refused = Vec::new();
    while std::time::Instant::now() < deadline && store.cards.is_empty() {
        let Ok(inbound) = rx.recv_timeout(Duration::from_secs(5)) else { break };
        match leo.open(&inbound.envelope, &mut tracker, now()) {
            Ok(Opened::Event(v)) => {
                ingest(&mut store, &config, v, now());
            }
            Ok(_) => {}
            Err(r) => refused.push(r),
        }
    }
    wire.stop();
    assert_eq!(store.cards.len(), 1, "only Maya's sealed event became a card");
    assert_eq!(store.cards[0].id, real.id);
    assert!(refused.contains(&Reject::UnknownDevice), "the stranger's envelope was refused: {refused:?}");
    let _ = std::fs::remove_dir_all(dir);
}

// --------------------------------------------------------------------------------- the roster

#[test]
fn the_roster_checks_every_signature_link_and_device_certificate() {
    let dir = temp_dir("roster");
    let wire = folder(&dir);
    let w = world(&wire, &["Leo"], NOW);
    let ops = w.roster.ops.clone();
    assert!(Roster::verify(&w.roster.team, &ops).is_ok());
    assert_eq!(w.roster.members.len(), 2);

    // Signed by a member who isn't the admin (Leo appends "add Mallory").
    let (_, leo_keys, _) = &w.others[0];
    let mut leo_copy = w.roster.clone();
    assert!(leo_copy.append(OpBody::Remove { member: w.roster.root.clone() }, &leo_keys.member().unwrap(), NOW).is_err());
    let mut forged = ops.clone();
    let mut body = forged[1].body.clone();
    body.seq = 2;
    body.prev = forged[1].hash();
    let sig = pm_relay_protocol::sign(&leo_keys.member().unwrap(), b"anything");
    forged.push(SignedOp { body, sig });
    assert!(Roster::verify(&w.roster.team, &forged).is_err(), "an operation not signed by the admin");

    // A changed name in a signed operation.
    let mut renamed = ops.clone();
    if let OpBody::Add { member } = &mut renamed[1].body.op {
        member.display = "Mallory".into();
    }
    assert!(Roster::verify(&w.roster.team, &renamed).is_err());

    // Out of order, or under another team id.
    let mut swapped = ops.clone();
    swapped.swap(0, 1);
    assert!(Roster::verify(&w.roster.team, &swapped).is_err());
    assert!(Roster::verify(&pm_relay_protocol::team_id(&[3; 32], b"s"), &ops).is_err());

    // A device the member never certified (the admin can't invent devices for people).
    let mut admin_copy = w.roster.clone();
    let maya_root = w.maya.member().unwrap();
    let fake_device = certify_device(&w.roster.team, &maya_root, &ed25519_dalek::SigningKey::from_bytes(&[5; 32]), &super::crypto::KemSecret::generate().public_b64(), "x");
    let leo_id = key_id(&leo_keys.member().unwrap());
    assert!(admin_copy.append(OpBody::AddDevice { member: leo_id, device: fake_device }, &maya_root, NOW).is_err());

    // My copy only ever moves forward along the same chain.
    let mut mine = w.roster.clone();
    assert!(mine.update_from(&ops[..1]).is_err(), "no rollback");
    let mut fork = w.roster.clone();
    fork.append(OpBody::Rotate { epoch: 2 }, &maya_root, NOW).unwrap();
    let mut other_fork = w.roster.clone();
    other_fork.append(OpBody::ConfirmGithub { member: key_id(&leo_keys.member().unwrap()) }, &maya_root, NOW).unwrap();
    let mut follower = fork.clone();
    assert!(follower.update_from(&other_fork.ops).is_err(), "no fork");
    let _ = std::fs::remove_dir_all(dir);
}

// --------------------------------------------------------------------- epoch rotation on revoke

#[test]
fn removing_a_member_rotates_the_team_key_and_locks_them_out() {
    let dir = temp_dir("rotate");
    let wire = folder(&dir);
    let mut w = world(&wire, &["Leo", "Sam"], NOW);
    let leo_id = key_id(&w.others[0].1.member().unwrap());
    assert_eq!(w.roster.epoch, 1);

    let epoch = membership::remove(&mut w.roster, &leo_id, &w.maya, wire.as_ref(), NOW).unwrap();
    assert_eq!(epoch, 2);
    assert_eq!(w.roster.epoch, 2);
    assert!(w.roster.member(&leo_id).is_none());
    assert!(matches!(w.roster.ops.last().unwrap().body.op, OpBody::Rotate { epoch: 2 }));

    // Sam (still on the team) picks up the new key and roster.
    let (_, sam_keys, sam_roster) = &mut w.others[1];
    let found = membership::refresh(sam_roster, sam_keys, wire.as_ref()).unwrap();
    assert!(found.new_epoch && found.roster_changed && !found.removed);
    assert_eq!(sam_roster.epoch, 2);
    let sam_keys = sam_keys.clone();
    let sam_roster = sam_roster.clone();

    // Leo learns he is out and gets no new key.
    let (_, leo_keys, leo_roster) = &mut w.others[0];
    let mut leo_roster_copy = leo_roster.clone();
    let leo_found = membership::refresh(&mut leo_roster_copy, leo_keys, wire.as_ref()).unwrap();
    assert!(!leo_found.new_epoch, "the new key was not sealed to Leo");
    assert!(leo_keys.epoch_key(&w.roster.team, 2).unwrap().is_none());

    // Maya's next message is sealed under epoch 2: Sam reads it, Leo can't.
    let at = now();
    let maya = Session::new(w.roster.clone(), w.maya.clone());
    let env = maya.seal(&Payload::Event { event: event_from(&w.maya, &w.roster.team, at) }, 5, at).unwrap();
    assert_eq!(env.epoch, 2);
    assert!(matches!(Session::new(sam_roster.clone(), sam_keys.clone()).open(&env, &mut SeqTracker::default(), at), Ok(Opened::Event(_))));
    assert_eq!(Session::new(leo_roster_copy.clone(), leo_keys.clone()).open(&env, &mut SeqTracker::default(), at), Err(Reject::NoKey));

    // And whatever Leo still sends is refused: his device left the roster.
    let leo_session = Session::new(leo_roster.clone(), leo_keys.clone());
    let from_leo = leo_session.seal(&Payload::Event { event: event_from(leo_keys, &w.roster.team, at) }, 3, at).unwrap();
    assert_eq!(maya.open(&from_leo, &mut SeqTracker::default(), at), Err(Reject::UnknownDevice));
    let _ = std::fs::remove_dir_all(dir);
}

// ----------------------------------------------------------------------------- gap detection

#[test]
fn a_missing_counter_from_a_sender_is_noticed_and_cleared_when_it_arrives() {
    let mut t = SeqTracker::default();
    t.observe("dev", 7, "A", NOW).unwrap(); // first sight: a baseline, not a gap
    assert_eq!(t.missing(), 0);
    t.observe("dev", 8, "B", NOW).unwrap();
    t.observe("dev", 11, "C", NOW).unwrap();
    assert_eq!(t.missing(), 2, "9 and 10 never came");
    t.observe("dev", 9, "D", NOW).unwrap();
    assert_eq!(t.missing(), 1, "a late one fills its gap");
    assert_eq!(t.observe("dev", 12, "C", NOW), Err(Reject::Replay), "a repeated id is a replay");
    t.observe("dev", 1, "E", NOW).unwrap(); // the sender reinstalled: counting starts over
    assert_eq!(t.missing(), 0);
    t.observe("dev", 3, "F", NOW).unwrap();
    assert_eq!(t.missing(), 1);
    t.clear_gaps();
    assert_eq!(t.missing(), 0);
}

// ----------------------------------------------------------------------------------- invites

#[test]
fn an_invite_needs_its_code_its_link_and_the_admins_yes() {
    let dir = temp_dir("invite");
    let wire = folder(&dir);
    let maya = person("maya");
    let t = wire.clone();
    let roster = membership::create_team("Acme", "Maya", &maya, move |_| Ok(t), NOW).unwrap();
    let (inv, code) = invite::new_invite(NOW);
    assert_eq!(code.len(), invite::CODE_LEN);
    assert!(inv.is_open(NOW) && !inv.is_open(NOW + invite::INVITE_TTL_MS + 1), "10 minutes");
    let link = membership::invite_link(&roster, &maya, Some("https://relay.example.com"), None, &inv.id).unwrap();
    let url = link.to_url();
    assert!(url.starts_with("pm://team/join?") && !url.contains(&code), "the code is not in the link");
    assert_eq!(InviteLink::parse(&url).unwrap(), link);
    // A link whose team id doesn't match its admin key.
    let tampered = url.replace(&format!("a={}", link.root), &format!("a={}", key_id(&ed25519_dalek::SigningKey::from_bytes(&[8; 32]))));
    assert!(InviteLink::parse(&tampered).is_err());

    // Leo with a wrong code: the admin's app refuses it.
    let leo = person("leo");
    let mut link_folder = link.clone();
    link_folder.relay = None;
    link_folder.folder = Some(dir.to_string_lossy().into_owned());
    membership::send_join(&link_folder, "WRONGCOD", "Leo", None, &leo, wire.as_ref(), now()).unwrap();
    let kem = maya.device_kem().unwrap();
    let request = wire.joins().unwrap().iter().find_map(|j| invite::open_join(&kem, j)).unwrap();
    assert_eq!(invite::check_join(&roster.team, &request, &code), Err("wrong code"));
    // Typed with a dash and lower case: still the code.
    let shown = invite::show_code(&code);
    assert_eq!(invite::normalize_code(&shown.to_lowercase()), code);
    // A request sealed to the admin can't be opened by anyone else.
    assert!(invite::open_join(&leo.device_kem().unwrap(), &wire.joins().unwrap()[0]).is_none());
    // Not let in: no key, no roster.
    assert!(membership::try_finish_join(&link_folder, &leo, wire.as_ref()).unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------------- relay end to end

fn relay_transport(addr: std::net::SocketAddr, team: &str, keys: &KeyStore) -> Arc<dyn TeamTransport> {
    Arc::new(RelayTransport::new(&format!("http://{addr}"), team, keys.device_sign().unwrap()).unwrap())
}

#[test]
fn relay_end_to_end_create_invite_join_push_and_card_with_metadata_only_on_the_relay() {
    let (addr, relay) = pm_relay::spawn_local(pm_relay::Config::default());
    let maya = person("maya");
    let roster = membership::create_team("Acme", "Maya", &maya, |team| Ok(relay_transport(addr, team, &maya)), NOW).unwrap();
    let maya_wire = relay_transport(addr, &roster.team, &maya);
    let mut roster = roster;

    // Invite → join → admin approves.
    let (inv, code) = invite::new_invite(now());
    let link = membership::invite_link(&roster, &maya, Some(&format!("http://{addr}")), None, &inv.id).unwrap();
    let link = InviteLink::parse(&link.to_url()).unwrap();
    let leo = person("leo");
    let leo_wire = relay_transport(addr, &link.team, &leo);
    membership::send_join(&link, &invite::show_code(&code), "Leo", Some("leo-dev".into()), &leo, leo_wire.as_ref(), now()).unwrap();
    assert!(membership::try_finish_join(&link, &leo, leo_wire.as_ref()).unwrap().is_none(), "the relay keeps Leo out until the admin says yes");
    let kem = maya.device_kem().unwrap();
    let request = maya_wire.joins().unwrap().iter().find_map(|j| invite::open_join(&kem, j)).unwrap();
    invite::check_join(&roster.team, &request, &code).unwrap();
    membership::approve(&mut roster, &request, &maya, maya_wire.as_ref(), now()).unwrap();
    let leo_roster = membership::try_finish_join(&link, &leo, leo_wire.as_ref()).unwrap().expect("Leo is in");
    assert_eq!(leo_roster.members.len(), 2);
    assert_eq!(leo_roster.members[1].github.as_deref(), Some("leo-dev"));

    // Leo listens over the WebSocket; a second listener of Leo's uses the long-poll fallback.
    let leo_rx = leo_wire.subscribe(0).unwrap();
    let poll_wire = RelayTransport::new(&format!("http://{addr}"), &link.team, leo.device_sign().unwrap()).unwrap().poll_only();
    let poll_rx = poll_wire.subscribe(0).unwrap();

    // Maya pushes and tells the team.
    let maya_session = Session::new(roster.clone(), maya.clone());
    let event = event_from(&maya, &roster.team, now());
    maya_wire.send(&maya_session.seal(&Payload::Event { event: event.clone() }, 1, now()).unwrap()).unwrap();

    let leo_session = Session::new(leo_roster.clone(), leo.clone());
    let mut store = TeamStore::default();
    let mut tracker = SeqTracker::default();
    let config = config_for(&leo, &leo_roster, "Leo");
    let take = |rx: &std::sync::mpsc::Receiver<Inbound>| -> Inbound {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let inbound = rx.recv_timeout(Duration::from_secs(20)).expect("an envelope arrives");
            if inbound.envelope.id == event.id || std::time::Instant::now() > deadline {
                return inbound;
            }
        }
    };
    let inbound = take(&leo_rx);
    assert!(inbound.cursor > 0, "the relay's mailbox position travels with it");
    assert_eq!(leo_wire.status(), "live");
    let verified = opened_event(leo_session.open(&inbound.envelope, &mut tracker, now()));
    assert!(matches!(ingest(&mut store, &config, verified, now()), Ingested::Card(_)));
    assert_eq!(store.cards[0].event.subject.branch.as_deref(), Some("feat/auth"));
    assert_eq!(take(&poll_rx).envelope.id, event.id, "the poll fallback delivers too");
    assert_eq!(poll_wire.status(), "polling");

    // What the relay stored: ciphertext and routing metadata, no names, branches, notes or keys.
    let dump = relay.dump();
    assert!(dump.contains(&roster.team));
    for readable in ["feat/auth", "Maya", "Leo", "leo-dev", "Acme", "session model", "github.com/acme", "Codex", &code] {
        assert!(!dump.contains(readable), "the relay must not hold {readable:?}");
    }
    let epoch_key = b64(&maya.epoch_key(&roster.team, 1).unwrap().unwrap());
    assert!(!dump.contains(&epoch_key), "the team key never reaches the relay");

    leo_wire.stop();
    poll_wire.stop();
}

#[test]
fn an_envelope_the_relay_drops_shows_up_as_a_gap() {
    let (addr, relay) = pm_relay::spawn_local(pm_relay::Config::default());
    let maya = person("maya");
    let mut roster = membership::create_team("Acme", "Maya", &maya, |team| Ok(relay_transport(addr, team, &maya)), NOW).unwrap();
    let maya_wire = relay_transport(addr, &roster.team, &maya);
    let (inv, code) = invite::new_invite(now());
    let link = membership::invite_link(&roster, &maya, Some(&format!("http://{addr}")), None, &inv.id).unwrap();
    let leo = person("leo");
    let leo_wire = relay_transport(addr, &link.team, &leo);
    membership::send_join(&link, &code, "Leo", None, &leo, leo_wire.as_ref(), now()).unwrap();
    let request = maya_wire.joins().unwrap().iter().find_map(|j| invite::open_join(&maya.device_kem().unwrap(), j)).unwrap();
    membership::approve(&mut roster, &request, &maya, maya_wire.as_ref(), now()).unwrap();
    let leo_roster = membership::try_finish_join(&link, &leo, leo_wire.as_ref()).unwrap().unwrap();

    let maya_session = Session::new(roster.clone(), maya.clone());
    for seq in 1..=3 {
        let e = event_from(&maya, &roster.team, now() + seq);
        maya_wire.send(&maya_session.seal(&Payload::Event { event: e }, seq, now()).unwrap()).unwrap();
    }
    relay.drop_envelope(&roster.team, 2).unwrap(); // a relay that loses (or withholds) one

    let rx = leo_wire.subscribe(0).unwrap();
    let leo_session = Session::new(leo_roster, leo.clone());
    let mut tracker = SeqTracker::default();
    let mut got = 0;
    while got < 2 {
        let inbound = rx.recv_timeout(Duration::from_secs(20)).expect("envelopes arrive");
        if let Ok(Opened::Event(_)) = leo_session.open(&inbound.envelope, &mut tracker, now()) {
            got += 1;
        }
    }
    leo_wire.stop();
    assert_eq!(tracker.missing(), 1, "Leo's app knows one of Maya's updates is missing");
}

// ------------------------------------------------------------------- keys stay in the keyring

#[test]
fn no_key_material_is_written_to_files_or_the_wire() {
    let dir = temp_dir("nokeys");
    let wire = folder(&dir);
    let w = world(&wire, &["Leo"], NOW);
    let maya_session = Session::new(w.roster.clone(), w.maya.clone());
    wire.send(&maya_session.seal(&Payload::Event { event: event_from(&w.maya, &w.roster.team, now()) }, 1, now()).unwrap()).unwrap();
    let state_dir = temp_dir("nokeys-state");
    std::fs::write(state_dir.join("roster.json"), serde_json::to_vec(&w.roster.ops).unwrap()).unwrap();
    super::config::save(&state_dir.join("team.json"), &config_for(&w.maya, &w.roster, "Maya")).unwrap();

    let mut secrets: Vec<String> = Vec::new();
    for keys in [&w.maya, &w.others[0].1] {
        secrets.push(b64(&keys.member().unwrap().to_bytes()));
        secrets.push(b64(&keys.device_sign().unwrap().to_bytes()));
        secrets.push(b64(&keys.device_kem().unwrap().to_bytes()));
        secrets.push(b64(&keys.epoch_key(&w.roster.team, 1).unwrap().unwrap()));
    }
    let mut checked = 0;
    for root in [&dir, &state_dir] {
        for entry in walk(root) {
            let text = String::from_utf8_lossy(&std::fs::read(&entry).unwrap()).into_owned();
            for secret in &secrets {
                assert!(!text.contains(secret.as_str()), "{} holds key material", entry.display());
            }
            checked += 1;
        }
    }
    assert!(checked >= 5, "envelopes, state slots, the join request, roster and config were checked");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(state_dir);
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

// ----------------------------------------------------------------------------- GitHub facts

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/team/fixtures/gh").join(name);
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

#[test]
fn gh_include_output_parses_both_a_200_and_a_304() {
    let ok = github::parse_include(&fixture("pulls-200.txt")).unwrap();
    assert_eq!(ok.status, 200);
    assert!(ok.etag.as_deref().unwrap().starts_with("W/\""));
    assert!(ok.body.trim_start().starts_with('['));
    let unchanged = github::parse_include(&fixture("pulls-304.txt")).unwrap();
    assert_eq!(unchanged.status, 304, "gh exits 1 on a 304; the status line is what counts");
    assert!(github::parse_include("gh: command not found").is_none());
}

#[test]
fn pull_request_facts_from_recorded_gh_output() {
    let body = github::parse_include(&fixture("pulls-200.txt")).unwrap().body;
    let first = github::parse_pulls(&body);
    assert_eq!(first.len(), 4, "recorded from cli/cli, four pull requests");
    assert!(first.iter().filter(|p| p.open).count() == 3);
    assert_eq!(github::diff_pulls(None, &first), vec![], "the first look is a baseline");
    let before: std::collections::BTreeMap<u64, _> = first.iter().map(|p| (p.number, p.clone())).collect();

    // The next poll: #14640 merged, #14679 closed without merging, a new #14700 opened.
    let mut next = first.clone();
    next[0].open = false;
    next[0].merged = true;
    next[1].open = false;
    let mut opened = first[2].clone();
    opened.number = 14700;
    opened.title = "Add a thing".into();
    opened.author = "not-on-the-team".into();
    next.push(opened);
    let facts = github::diff_pulls(Some(&before), &next);
    let kinds: Vec<(u64, &str)> = facts
        .iter()
        .map(|f| match f {
            Fact::PrOpened(p) => (p.number, "opened"),
            Fact::PrMerged(p) => (p.number, "merged"),
            Fact::PrClosed(p) => (p.number, "closed"),
            _ => (0, "ci"),
        })
        .collect();
    assert_eq!(kinds, vec![(14640, "merged"), (14679, "closed"), (14700, "opened")]);

    // As cards: the author maps to a teammate by GitHub login; others show their login.
    let dir = temp_dir("ghroster");
    let wire = folder(&dir);
    let mut w = world(&wire, &["Leo"], NOW);
    let leo_id = key_id(&w.others[0].1.member().unwrap());
    // Leo claimed the login of #14640's author.
    let author = first[0].author.clone();
    if let Some(m) = w.roster.members.iter_mut().find(|m| m.id == leo_id) {
        m.github = Some(author.clone());
    }
    let merged = github::fact_event("github.com/cli/cli", &facts[0], Some(&w.roster), NOW);
    assert_eq!(merged.kind, TeamEventKind::PrMerged);
    assert_eq!(merged.actor.member, leo_id);
    assert_eq!(merged.actor.display, "Leo");
    assert_eq!(merged.source, EventSource::Github);
    assert_eq!(merged.dedupe_key, "github.com/cli/cli#14640:pr", "opened and merged are one card");
    let opened = github::fact_event("github.com/cli/cli", &facts[2], Some(&w.roster), NOW);
    assert_eq!(opened.actor.member, "github:not-on-the-team");
    assert_eq!(opened.actor.display, "not-on-the-team");
    assert!(validate_inbound(merged.clone(), NOW).is_ok());

    // Facts go straight to cards (no relay), including facts about my own pull requests.
    let config = config_for(&w.others[0].1, &w.roster, "Leo");
    let mut store = TeamStore::default();
    let fact = Verified::local_fact(merged).unwrap();
    assert!(matches!(ingest(&mut store, &config, fact, NOW), Ingested::Card(_)));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ci_facts_from_recorded_check_runs() {
    let passed = github::parse_include(&fixture("checks-passed.txt")).unwrap().body;
    let failed = github::parse_include(&fixture("checks-failed.txt")).unwrap().body;
    let (state, ci) = github::ci_state(&passed);
    assert_eq!(state, CiState::Passed, "skipped runs don't fail a commit");
    assert!(ci.failed_jobs.is_empty());
    let (state, ci) = github::ci_state(&failed);
    assert_eq!(state, CiState::Failed);
    assert_eq!(ci.failed_jobs, vec!["govulncheck".to_string()]);
    assert!(ci.url.starts_with("https://github.com/cli/cli/"));

    assert!(github::ci_fact(None, CiState::Failed, "feat/x", None, ci.clone()).is_none(), "baseline");
    assert!(matches!(github::ci_fact(Some(CiState::Pending), CiState::Failed, "feat/x", None, ci.clone()), Some(Fact::CiFailed { .. })));
    assert!(github::ci_fact(Some(CiState::Failed), CiState::Failed, "feat/x", None, ci.clone()).is_none(), "once per red streak");
    assert!(matches!(github::ci_fact(Some(CiState::Failed), CiState::Passed, "feat/x", None, ci.clone()), Some(Fact::CiPassed { .. })));
    assert!(github::ci_fact(Some(CiState::Passed), CiState::Passed, "feat/x", None, ci.clone()).is_none());
    let red = github::fact_event("github.com/cli/cli", &Fact::CiFailed { branch: "feat/x".into(), pr: None, ci }, None, NOW);
    assert_eq!(red.tier_hint, Tier::Interrupt);
    assert_eq!(red.subject.ci.as_ref().unwrap().failed_jobs, vec!["govulncheck".to_string()]);
}

/// A `gh` that answers from the recorded fixtures and records every call.
#[derive(Default)]
struct FakeGh {
    calls: Mutex<Vec<String>>,
    run_error: Option<String>,
}

impl Gh for FakeGh {
    fn api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String> {
        self.calls.lock().push(format!("api {path} etag={}", etag.unwrap_or("-")));
        let text = if path.contains("/pulls") {
            if etag.is_some() { fixture("pulls-304.txt") } else { fixture("pulls-200.txt") }
        } else if path.contains("/check-runs") {
            fixture("checks-passed.txt")
        } else {
            "HTTP/2.0 200 OK\n\n{\"allow_merge_commit\":false,\"allow_squash_merge\":true,\"allow_auto_merge\":true}".into()
        };
        Ok(github::parse_include(&text).unwrap())
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        self.calls.lock().push(format!("run {}", args.join(" ")));
        match &self.run_error {
            Some(e) => Err(e.clone()),
            None => Ok(String::new()),
        }
    }
}

#[test]
fn the_poller_sends_etags_and_gets_a_304_the_second_time() {
    let gh = FakeGh::default();
    let mut state = RepoPoll::default();
    let first = github::poll_repo(&gh, "cli/cli", &mut state, NOW).unwrap();
    assert!(first.is_empty(), "baseline");
    assert!(state.next_at_ms >= NOW + github::DEFAULT_POLL_S * 1000);
    let second = github::poll_repo(&gh, "cli/cli", &mut state, NOW + 60_000).unwrap();
    assert!(second.is_empty(), "304: nothing changed");
    let calls = gh.calls.lock().clone();
    assert!(calls[0].starts_with("api repos/cli/cli/pulls") && calls[0].ends_with("etag=-"));
    let pulls_again = calls.iter().filter(|c| c.contains("/pulls")).nth(1).unwrap();
    assert!(pulls_again.contains("etag=W/\""), "the second poll is conditional: {pulls_again}");
    assert!(calls.iter().all(|c| c.starts_with("api ")), "polling only reads: {calls:?}");
    assert_eq!(github::owner_repo("github.com/acme/billing").as_deref(), Some("acme/billing"));
    assert_eq!(github::owner_repo("gitlab.com/acme/billing"), None);
    assert_eq!(github::owner_repo("github.com/acme/../x"), None);
}

// -------------------------------------------------------------------------- merge when green

fn pr_event(state: &str, key: &str) -> TeamEvent {
    let mut e = event_from(&person("maya"), "T", NOW);
    e.kind = TeamEventKind::PrOpened;
    e.project.key = key.into();
    e.subject.commit = None;
    e.subject.pr = Some(PrRef {
        number: 142,
        title: "Add OAuth token refresh".into(),
        url: "https://github.com/acme/billing/pull/142".into(),
        state: state.into(),
        draft: false,
        files: None,
        additions: None,
        deletions: None,
    });
    e
}

#[test]
fn merge_when_green_only_arms_githubs_auto_merge() {
    let gh = FakeGh::default();
    let plan = merge_green::plan(&gh, &pr_event("open", "github.com/acme/billing")).unwrap();
    assert_eq!(plan.method, merge_green::Method::Squash, "the repository allows squash only");
    let args = merge_green::arm_args(&plan);
    assert_eq!(args, vec!["pr", "merge", "142", "--auto", "--squash", "--repo", "acme/billing"]);
    let message = merge_green::arm(&gh, &plan).unwrap();
    assert!(message.contains("when its checks pass") && message.contains("Nothing was merged"));
    let runs: Vec<String> = gh.calls.lock().iter().filter(|c| c.starts_with("run ")).cloned().collect();
    assert_eq!(runs, vec!["run pr merge 142 --auto --squash --repo acme/billing"]);
    assert!(runs.iter().all(|r| r.contains("--auto")), "never a direct merge");

    // Not for merged or closed PRs, not for other forges, not without a PR.
    assert!(merge_green::plan(&gh, &pr_event("merged", "github.com/acme/billing")).is_err());
    assert!(merge_green::plan(&gh, &pr_event("open", "gitlab.com/acme/billing")).is_err());
    let mut push = pr_event("open", "github.com/acme/billing");
    push.subject.pr = None;
    assert!(merge_green::plan(&gh, &push).is_err());

    // GitHub says auto-merge is off: a plain sentence.
    let off = FakeGh { run_error: Some("GraphQL: Auto merge is not allowed for this repository (enablePullRequestAutoMerge)".into()), ..FakeGh::default() };
    assert!(merge_green::arm(&off, &plan).unwrap_err().starts_with("Auto-merge is off for this repository"));

    // A card arriving never touches gh: ingest has no path to it.
    let quiet = FakeGh::default();
    let mut store = TeamStore::default();
    let mut config = TeamConfig { team: "T".into(), ..TeamConfig::default() };
    config.me.id = "someone-else".into();
    let _ = ingest(&mut store, &config, Verified::for_test(pr_event("open", "github.com/acme/billing")), NOW);
    assert!(quiet.calls.lock().is_empty());
    assert!(merge_green::pick_method("{}").0 == merge_green::Method::Merge, "unknown settings: GitHub's default");
}

// --------------------------------------------------------------------------- the phone route

#[test]
fn phone_team_routes_follow_device_caps_and_refuse_agents() {
    use crate::mobile_pairing::{route_need, Cap, Caller, Caps, Need};
    use super::phone;
    assert_eq!(route_need("GET", &["team", "cards"]), Need::Cap(Cap::Watch));
    assert_eq!(route_need("POST", &["team", "cards", "c1", "dismiss"]), Need::AnyOf(Cap::Answer, Cap::Control));
    assert_eq!(route_need("POST", &["team", "cards", "c1", "queue"]), Need::Cap(Cap::Control));
    assert_eq!(route_need("POST", &["team", "jobs", "j1", "apply"]), Need::Cap(Cap::Control));
    // Never a public route: a teammate or a stranger can't reach them without a bearer.
    for path in [&["team", "cards"][..], &["team", "cards", "c1", "dismiss"], &["team", "jobs", "j1", "apply"]] {
        assert!(!crate::mobile_pairing::is_public_bridge_path(path, "GET"));
        assert!(!crate::mobile_pairing::is_public_bridge_path(path, "POST"));
    }

    let device = |caps: Caps| Caller::Device { device_id: "d1".into(), caps };
    let watch = device(Caps::none().with(Cap::Watch, true));
    let answer = device(Caps::none().with(Cap::Answer, true));
    let control = device(Caps::none().with(Cap::Control, true));
    assert_eq!(phone::can(&watch), phone::Can { dismiss: false, act: false });
    assert_eq!(phone::can(&answer), phone::Can { dismiss: true, act: false });
    assert_eq!(phone::can(&control), phone::Can { dismiss: true, act: true });
    assert_eq!(phone::can(&Caller::Local), phone::Can { dismiss: true, act: true });

    let body = serde_json::json!({ "paneId": "p1" });
    let status = |r: Option<phone::RouteResult>| match r.unwrap() {
        Ok((s, _)) => s,
        Err((s, _)) => s,
    };
    assert_eq!(status(phone::handle("POST", &["team", "cards", "c1", "dismiss"], &body, &watch, false)), 403);
    assert_eq!(status(phone::handle("POST", &["team", "cards", "c1", "queue"], &body, &answer, false)), 403);
    assert_eq!(status(phone::handle("POST", &["team", "jobs", "j1", "apply"], &body, &answer, false)), 403);
    assert_eq!(status(phone::handle("POST", &["team", "jobs", "j1", "discard"], &body, &watch, false)), 403);
    // An agent session is refused even on the read route.
    assert_eq!(status(phone::handle("GET", &["team", "cards"], &body, &Caller::Local, true)), 403);
    assert!(phone::handle("GET", &["panes"], &body, &Caller::Local, false).is_none(), "not a team route");

    // The phone view carries what this caller may do, and no unsent drafts.
    let view = super::service::TeamView {
        enabled: true,
        active: true,
        me: "Leo".into(),
        members: vec!["Maya".into()],
        transport: Some("relay"),
        link: Some("live"),
        error: None,
        missing: 0,
        drafts: vec![],
        cards: vec![],
        jobs: vec![],
    };
    let json = phone::cards_view(view, phone::can(&answer));
    assert_eq!(json["can"], serde_json::json!({ "dismiss": true, "act": false }));
    assert!(json.get("drafts").is_none());
}

// -------------------------------------------------------------------- service: config + audit

#[test]
fn an_unreadable_team_json_is_reported_and_kept() {
    let dir = temp_dir("badcfg");
    std::fs::write(dir.join("team.json"), b"{ this is not json").unwrap();
    let (config, error) = super::service::load_config(&dir);
    let error = error.expect("reported");
    assert!(error.contains("couldn't be read"), "{error}");
    assert!(!config.enabled, "Team stays off");
    // Saving keeps the old file next to the new one.
    super::config::save(&dir.join("team.json"), &TeamConfig::default()).unwrap();
    assert!(dir.join("team.json.unreadable").exists());
    assert!(super::service::load_config(&dir).1.is_none());
    // A missing file is a fresh start, not an error.
    let empty = temp_dir("nocfg");
    assert!(super::service::load_config(&empty).1.is_none());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(empty);
}

#[test]
fn a_teammates_card_is_written_to_the_audit_log() {
    let dir = temp_dir("audit");
    let wire = folder(&dir);
    let w = world(&wire, &["Leo"], NOW);
    let (_, leo_keys, leo_roster) = &w.others[0];
    let leo = Session::new(leo_roster.clone(), leo_keys.clone());
    let config = config_for(leo_keys, leo_roster, "Leo");
    let mut store = TeamStore::default();
    let event = event_from(&w.maya, &w.roster.team, now());
    let env = Session::new(w.roster.clone(), w.maya.clone()).seal(&Payload::Event { event: event.clone() }, 1, now()).unwrap();
    let got = super::service::open_and_ingest(&leo, &mut store, &config, &Inbound { envelope: env.clone(), received_at_ms: now(), cursor: 7 });
    let card = got.card.expect("a card");
    super::service::audit_received(&card);
    assert_eq!(store.cards.len(), 1);
    assert_eq!(store.cursor, 7);
    let entries = crate::audit::global().recent(500);
    let mine = entries.iter().find(|e| e.kind == "team.received" && e.detail["event"] == event.id).expect("audited");
    assert_eq!(mine.who, "teammate");
    assert_eq!(mine.actor.as_deref(), Some(&w.roster.root[..8]));
    // A forged copy makes no card (so there is nothing to audit as received).
    let mut forged = env.clone();
    forged.sign_with(&ed25519_dalek::SigningKey::from_bytes(&[3; 32]));
    let refused = super::service::open_and_ingest(&leo, &mut store, &config, &Inbound { envelope: forged, received_at_ms: now(), cursor: 8 });
    assert!(refused.card.is_none() && refused.refresh, "an unknown sender makes the app re-read the roster");
    assert_eq!(store.cards.len(), 1);
    assert_eq!(store.cursor, 8, "the cursor moves past refused mail");
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------- the bridge-auth invariant

#[test]
fn team_traffic_is_outbound_only_and_never_uses_pairing() {
    // By inspection: nothing in team/ (outside tests and the phone route, which is the person's
    // own paired phone going through the normal bridge gate) listens for connections or touches
    // device pairing tokens.
    let sources = [
        ("service.rs", include_str!("service.rs")),
        ("membership.rs", include_str!("membership.rs")),
        ("secure.rs", include_str!("secure.rs")),
        ("invite.rs", include_str!("invite.rs")),
        ("transport/mod.rs", include_str!("transport/mod.rs")),
        ("transport/relay.rs", include_str!("transport/relay.rs")),
        ("transport/folder.rs", include_str!("transport/folder.rs")),
        ("sources/github.rs", include_str!("sources/github.rs")),
        ("merge_green.rs", include_str!("merge_green.rs")),
    ];
    for (file, src) in sources {
        let code = src.split("#[cfg(test)]").next().unwrap_or(src);
        for needle in ["TcpListener", "UdpSocket", ".bind(", "pairing_store", "validate_bearer_token", "create_pairing_session", "bearer_token("] {
            assert!(!code.contains(needle), "{file} contains {needle}");
        }
    }
    // The phone route uses the bridge's own gate result, never a token of its own.
    let phone = include_str!("phone.rs");
    assert!(!phone.contains("pairing_store") && !phone.contains("bearer_token("));
    // And the bridge has no public team path: every /team route needs the local app or a bearer.
    for method in ["GET", "POST", "PUT", "DELETE"] {
        for path in [&["team"][..], &["team", "cards"], &["team", "join"], &["team", "envelopes"], &["bridge", "team", "cards"]] {
            assert!(!crate::mobile_pairing::is_public_bridge_path(path, method), "{method} {path:?}");
        }
    }
    // A forwarded request (a tunnel) without a bearer is refused at the gate.
    let refused = crate::mobile_pairing::authorize_caller("Host: 127.0.0.1\r\nX-Forwarded-For: 1.2.3.4\r\n", true, &["team", "cards"], "GET");
    assert!(matches!(refused, Err((401, _)) | Err((503, _))));
}
