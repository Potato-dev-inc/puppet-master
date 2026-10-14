//! Tests for the co-work security review fixes (`D:\pm-night\SECURITY-REVIEW-COWORK.md`): each
//! one failed before its fix.

use super::config::{Member, TeamConfig};
use super::crypto::key_id;
use super::event::*;
use super::invite::{self, InviteLink};
use super::keystore::KeyStore;
use super::membership;
use super::roster::Roster;
use super::secure::{Payload, Session};
use super::transport::relay::RelayTransport;
use super::transport::{Inbound, TeamTransport};
use crate::secrets::MemoryBackend;
use std::sync::Arc;
use std::time::Duration;

const NOW: u64 = 1_790_000_000_000;

fn now() -> u64 {
    super::transport::now_ms()
}

fn person(name: &str) -> KeyStore {
    KeyStore::with_backend(Arc::new(MemoryBackend::new()), name)
}

fn relay_transport(addr: std::net::SocketAddr, team: &str, keys: &KeyStore) -> Arc<dyn TeamTransport> {
    Arc::new(RelayTransport::new(&format!("http://{addr}"), team, keys.device_sign().unwrap()).unwrap())
}

/// Maya's team on a local relay, with Leo let in. Returns (relay address, relay, Maya's roster,
/// Maya, Leo, Leo's roster).
struct RelayTeam {
    addr: std::net::SocketAddr,
    relay: Arc<pm_relay::Relay>,
    roster: Roster,
    maya: KeyStore,
    maya_wire: Arc<dyn TeamTransport>,
    leo: KeyStore,
    leo_roster: Roster,
}

fn relay_team() -> RelayTeam {
    let (addr, relay) = pm_relay::spawn_local(pm_relay::Config::default());
    let maya = person("maya");
    let mut roster = membership::create_team("Acme", "Maya", &maya, |team| Ok(relay_transport(addr, team, &maya)), NOW).unwrap();
    let maya_wire = relay_transport(addr, &roster.team, &maya);
    let (inv, code) = invite::new_invite(now());
    let link = membership::invite_link(&roster, &maya, Some(&format!("http://{addr}")), None, &inv.id).unwrap();
    let link = InviteLink::parse(&link.to_url()).unwrap();
    let leo = person("leo");
    let leo_wire = relay_transport(addr, &link.team, &leo);
    membership::send_join(&link, &code, "Leo", None, &leo, leo_wire.as_ref(), now()).unwrap();
    let request = maya_wire.joins().unwrap().iter().find_map(|j| invite::open_join(&maya.device_kem().unwrap(), j)).unwrap();
    membership::approve(&mut roster, &request, &maya, maya_wire.as_ref(), now()).unwrap();
    let leo_roster = membership::try_finish_join(&link, &leo, leo_wire.as_ref()).unwrap().unwrap();
    RelayTeam { addr, relay, roster, maya, maya_wire, leo, leo_roster }
}

fn push_event(keys: &KeyStore, team: &str, at: u64) -> TeamEvent {
    let subject = Subject {
        branch: Some("feat/auth".into()),
        commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
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
            agent: None,
        },
        kind: TeamEventKind::Pushed,
        at_ms: at,
        dedupe_key: dedupe_key("github.com/acme/billing", TeamEventKind::Pushed, &subject),
        subject,
        note: None,
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

fn send_events(t: &RelayTeam, n: u64, first_seq: u64) -> Vec<String> {
    let session = Session::new(t.roster.clone(), t.maya.clone());
    (0..n)
        .map(|i| {
            let event = push_event(&t.maya, &t.roster.team, now() + i);
            let id = event.id.clone();
            t.maya_wire.send(&session.seal(&Payload::Event { event }, first_seq + i, now()).unwrap()).unwrap();
            id
        })
        .collect()
}

fn drain(rx: &std::sync::mpsc::Receiver<Inbound>, quiet: Duration) -> Vec<Inbound> {
    let mut out = Vec::new();
    while let Ok(inbound) = rx.recv_timeout(quiet) {
        out.push(inbound);
    }
    out
}

// ------------------------------------------------------------------------- M1: the relay cursor

/// The app's cursor is past the relay's counter (a restored or replaced relay). Before the fix the
/// client kept asking `after=812`: every poll re-read the whole mailbox (each envelope delivered
/// again and again), and the saved cursor never came back down, so after a restart it would skip
/// the relay's new envelopes 1..812.
#[test]
fn a_client_whose_cursor_is_past_the_relay_starts_again_from_what_the_relay_has() {
    let t = relay_team();
    send_events(&t, 2, 1);
    let wire = RelayTransport::new(&format!("http://{}", t.addr), &t.roster.team, t.leo.device_sign().unwrap()).unwrap().poll_only();
    let rx = wire.subscribe(812).unwrap();
    let first = drain(&rx, Duration::from_secs(3));
    // Mail sent after the re-sync arrives once, and nothing is delivered twice.
    let later = send_events(&t, 1, 3);
    let second = drain(&rx, Duration::from_secs(3));
    wire.stop();
    let all: Vec<&Inbound> = first.iter().chain(second.iter()).collect();
    let ids: Vec<&str> = all.iter().map(|i| i.envelope.id.as_str()).collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(ids.len(), unique.len(), "no envelope is delivered twice ({} deliveries of {} envelopes)", ids.len(), unique.len());
    assert!(ids.contains(&later[0].as_str()), "mail after the re-sync arrives");
    assert!(first.first().is_some_and(|i| i.reset), "the first envelope after the re-sync says so");

    // The service takes the lower cursor (it used to keep the highest it had ever seen).
    let leo_session = Session::new(t.leo_roster.clone(), t.leo.clone());
    let mut store = super::store::TeamStore { cursor: 812, ..Default::default() };
    let config = config_for(&t.leo, &t.leo_roster, "Leo");
    for inbound in &all {
        super::service::open_and_ingest(&leo_session, &mut store, &config, inbound);
    }
    assert!(store.cursor < 812 && store.cursor >= all.last().unwrap().cursor, "saved cursor {} follows the relay", store.cursor);
    assert_eq!(store.cards.len(), 1, "the three pushes of one branch are one card");
}

/// A quiet week empties the relay's mailbox; a teammate's next envelope still reaches a client
/// holding the old cursor (over the WebSocket and the long poll).
#[test]
fn after_a_quiet_week_the_next_envelope_still_arrives() {
    let t = relay_team();
    send_events(&t, 3, 1);
    let leo_wire = relay_transport(t.addr, &t.roster.team, &t.leo);
    let rx = leo_wire.subscribe(0).unwrap();
    let seen = drain(&rx, Duration::from_secs(3));
    let cursor = seen.iter().map(|i| i.cursor).max().unwrap();
    leo_wire.stop();
    t.relay.sweep_at(now() + pm_relay_protocol::MAILBOX_TTL_MS + 60_000).unwrap();
    let ids = send_events(&t, 1, 4);
    for poll_only in [false, true] {
        let mut wire = RelayTransport::new(&format!("http://{}", t.addr), &t.roster.team, t.leo.device_sign().unwrap()).unwrap();
        if poll_only {
            wire = wire.poll_only();
        }
        let rx = wire.subscribe(cursor).unwrap();
        let got = rx.recv_timeout(Duration::from_secs(20)).expect("the new envelope arrives");
        assert_eq!(got.envelope.id, ids[0]);
        assert!(got.cursor > cursor);
        wire.stop();
    }
}
