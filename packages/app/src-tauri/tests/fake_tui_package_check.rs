//! Package check, end to end (SPEC n33 section 5, PC-14): zero tokens, `pm-fake-tui` as the agent on a real ConPTY, the
//! real card (risk, packages), the real lookup (`package_check::live::lookup`: settings, private-server check, cache,
//! thread pool, deadline, `UreqFetcher`, registry parsers, sentences). Only the agent and the registry are fake: the
//! registry is a server on `127.0.0.1` on a port the OS picks, so nothing leaves the machine.
//!
//! 1. `npm install lodahs`: the card is High ("look-alike name") and its first paint says `Looks like lodash`; the
//!    lookup adds the facts the registry gave; the registry saw only the package's two paths and only the headers
//!    User-Agent, Accept (and the client's own Host / Accept-Encoding); the agent was never answered; the card is the
//!    same before and after.
//! 2. The registry is gone: `Could not check: npm could not be reached`, status `Could not check`, buttons unchanged.
//! 3. A slow registry: the card comes back within the deadline with `did not answer in time`.
//! 4. The project has its own package server for `@acme`: that name is never sent, the other one is looked up.
//! 5. The setting is off: every row says so and nothing is requested.
//! 6. A prompt id that is not on the screen (or a pane that is not there) is `STALE_PROMPT`; nothing is requested.
//! 7. A secret typed as a package name: counted in `hidden`, never shown, never looked up.
//! 8. Two ecosystems on one line: two rows, each from its own registry.
//!
//! The tests never answer a card. Included from `fake_tui_e2e.rs` (one test binary, one World per process).

use puppet_master_app_lib::fake_tui::{PackageRig, Session, World};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const STOPPED: &[&str] = &["completed", "failed", "cancelled", "needs_input"];
/// `package_check::model::DEADLINE_MS`.
const DEADLINE: Duration = Duration::from_millis(8_000);

// ---- helpers ---------------------------------------------------------------------------------------------------

/// A fake Claude that asks to run `command` and waits for the person.
fn script(command: &str) -> String {
    format!(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  {{ do = "think", ms = 300 }},
  {{ do = "approve", kind = "bash", command = "{command}", on_allow = [{{ do = "say", text = "Installed." }}], on_deny = [{{ do = "say", text = "Stopped." }}] }},
]
"#
    )
}

/// Runs the fake until it shows its Bash approval. Returns the pane and the card the desktop would render.
fn park(session: &Session, command: &str) -> (String, Value) {
    session.script(&script(command));
    let started = session.run("fake_claude", "Install the package.", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let handle = started["handle"].as_str().expect("a handle").to_string();
    let parked = session.wait(&handle, STOPPED, 90_000).unwrap_or_else(|e| panic!("wait failed: {e}"));
    assert_eq!(parked["status"], "needs_input", "the run should be waiting on the approval: {parked}\n{}", session.events().len());
    let pane = parked["pane_id"].as_str().expect("a pane").to_string();
    let card = session.card(&pane).unwrap_or_else(|| panic!("the pane shows no card:\n{}", session.screen(&pane)));
    assert_eq!(card["kind"], "command_approval", "{card}");
    (pane, card)
}

fn prompt_id(card: &Value) -> String {
    card["prompt_id"].as_str().expect("a prompt id").to_string()
}

fn rows(view: &Value) -> &Vec<Value> {
    view["rows"].as_array().unwrap_or_else(|| panic!("no rows in {view}"))
}

/// Everything one row says: its summary and every fact line.
fn row_text(row: &Value) -> String {
    let mut text = row["summary"].as_str().unwrap_or_default().to_string();
    for line in row["lines"].as_array().into_iter().flatten() {
        text.push('\n');
        text.push_str(line["text"].as_str().unwrap_or_default());
    }
    text
}

/// The parts of a card a lookup must never change (S8): the id, the buttons, the level and the headline.
fn stable_parts(card: &Value) -> Value {
    json!({
        "prompt_id": card["prompt_id"], "kind": card["kind"], "options": card["options"], "choices": card["choices"],
        "level": card["risk"]["level"], "headline": card["risk"]["headline"],
    })
}

/// Nothing reached the agent after it drew its approval: no key, no answer, no text (the Enter that sent the task
/// came before).
fn nothing_was_answered(session: &Session) {
    let events = session.events();
    let shown = events.iter().position(|event| event["ev"] == "approval_shown").expect("the agent drew its approval");
    let after = &events[shown + 1..];
    for ev in ["approval_answer", "key", "prompt", "gate_answer", "typed_at_gate"] {
        assert!(after.iter().all(|event| event["ev"] != ev), "the agent saw `{ev}` after its approval: {events:#?}");
    }
}

// ---- 1. a look-alike name, the facts, and what left the machine -------------------------------------------------------

#[test]
fn a_look_alike_install_shows_the_registry_facts_and_sends_only_the_name() {
    let session = World::get().session("pc-lookalike");
    let rig = PackageRig::start();
    rig.serve_npm("lodahs", 3, 12, true);
    let (pane, card) = park(&session, "npm install lodahs");

    assert_eq!(card["risk"]["level"], "high", "{card}");
    assert_eq!(card["risk"]["headline"], "Install a package with a look-alike name", "{card}");
    let items = card["packages"]["items"].as_array().expect("packages on the card");
    assert_eq!(items.len(), 1, "{card}");
    assert_eq!((items[0]["ecosystem"].as_str(), items[0]["name"].as_str()), (Some("npm"), Some("lodahs")), "{card}");
    let first = items[0]["first"].as_str().unwrap_or_default();
    assert!(first.starts_with("Looks like lodash"), "first paint: {first}");
    assert!(rig.requests().is_empty(), "drawing the card made a request: {:?}", rig.requests());

    let view = rig.lookup(&pane, &prompt_id(&card), false).unwrap_or_else(|e| panic!("lookup failed: {e}"));
    let all = rows(&view).iter().map(row_text).collect::<Vec<_>>().join("\n");
    for fact in ["Looks like lodash", "New: first published 3 days ago", "Few downloads: 12 a week", "Runs a script when installed"] {
        assert!(all.contains(fact), "`{fact}` is missing from:\n{all}\n{view}");
    }
    assert_eq!(rows(&view).len(), 1, "{view}");
    assert_eq!(rows(&view)[0]["state"], "checked", "{view}");
    assert_eq!(rows(&view)[0]["look"], true, "{view}");

    // What reached the registry: the two paths of this one package, with the client's four headers and nothing else.
    let mut paths = rig.paths();
    paths.sort();
    assert_eq!(paths, ["/downloads/point/last-week/lodahs", "/lodahs"], "{:?}", rig.requests());
    let project = session.project().to_string_lossy().to_lowercase();
    for seen in rig.requests() {
        assert_eq!(seen.header_names(), ["accept", "accept-encoding", "host", "user-agent"], "{seen:?}");
        let agent = seen.headers.iter().find(|(name, _)| name == "user-agent").map(|(_, value)| value.as_str()).unwrap_or_default();
        assert!(agent.starts_with("PuppetMaster-PackageCheck/"), "{seen:?}");
        assert_eq!(seen.headers.iter().find(|(name, _)| name == "accept").map(|(_, v)| v.as_str()), Some("application/json"), "{seen:?}");
        let sent = format!("{} {:?}", seen.request_line, seen.headers).to_lowercase();
        assert!(!sent.contains(&project) && !sent.contains(&pane.to_lowercase()), "project data in the request: {seen:?}");
    }

    // The facts changed nothing on the card, and nobody answered it.
    let after = session.card(&pane).expect("the card is still up");
    assert_eq!(stable_parts(&after), stable_parts(&card), "the lookup changed the card");
    assert_eq!(after["packages"], card["packages"], "the first paint changed");
    nothing_was_answered(&session);
}

// ---- 2. the registry is gone --------------------------------------------------------------------------------------

#[test]
fn a_registry_that_cannot_be_reached_says_so_and_leaves_the_buttons_alone() {
    let session = World::get().session("pc-closed");
    let rig = PackageRig::start();
    rig.serve_npm("zinc-closed-tool", 400, 50_000, false);
    rig.close();
    let (pane, card) = park(&session, "npm install zinc-closed-tool");

    let view = rig.lookup(&pane, &prompt_id(&card), false).unwrap_or_else(|e| panic!("lookup failed: {e}"));
    let text = row_text(&rows(&view)[0]);
    assert!(text.contains("Could not check: npm could not be reached"), "{text}\n{view}");
    assert_eq!(rows(&view)[0]["state"], "failed", "{view}");
    assert_eq!(view["statusText"], "Could not check", "{view}");
    assert_eq!(view["canRetry"], true, "{view}");

    let after = session.card(&pane).expect("the card is still up");
    assert_eq!(stable_parts(&after), stable_parts(&card), "a failed lookup changed the card");
    nothing_was_answered(&session);
}

// ---- 3. a slow registry -------------------------------------------------------------------------------------------

#[test]
fn a_slow_registry_is_given_up_on_within_the_deadline() {
    let session = World::get().session("pc-slow");
    let rig = PackageRig::start();
    rig.serve_npm("zinc-slow-tool", 400, 50_000, false);
    rig.slow(Duration::from_secs(20));
    let (pane, card) = park(&session, "npm install zinc-slow-tool");

    let started = Instant::now();
    let view = rig.lookup(&pane, &prompt_id(&card), false).unwrap_or_else(|e| panic!("lookup failed: {e}"));
    let took = started.elapsed();
    assert!(took <= DEADLINE + Duration::from_secs(1), "the card waited {took:?}");
    let text = row_text(&rows(&view)[0]);
    assert!(text.contains("did not answer in time"), "{text}\n{view}");
    assert_eq!(rows(&view)[0]["state"], "failed", "{view}");
    assert_eq!(view["statusText"], "Could not check", "{view}");
    nothing_was_answered(&session);
}

// ---- 4. the project's own package server -----------------------------------------------------------------------------

#[test]
fn a_name_on_the_projects_own_server_is_never_sent_to_the_public_registry() {
    let session = World::get().session("pc-private");
    let rig = PackageRig::start();
    rig.serve_npm("left-pad", 3000, 2_000_000, false);
    std::fs::write(session.project().join(".npmrc"), "@acme:registry=https://npm.acme.test/\n").expect("write .npmrc");
    let (pane, card) = park(&session, "npm i @acme/tools left-pad");

    let view = rig.lookup(&pane, &prompt_id(&card), false).unwrap_or_else(|e| panic!("lookup failed: {e}"));
    assert_eq!(rows(&view).len(), 2, "{view}");
    let acme = &rows(&view)[0];
    assert_eq!(acme["name"], "@acme/tools", "{view}");
    assert_eq!(acme["state"], "not_checked", "{view}");
    assert!(row_text(acme).contains("Not checked: this project uses its own package server"), "{view}");
    assert_eq!(rows(&view)[1]["name"], "left-pad", "{view}");
    assert_eq!(rows(&view)[1]["state"], "checked", "{view}");

    let seen = rig.requests();
    assert!(!seen.is_empty(), "left-pad was never looked up");
    for request in &seen {
        let sent = format!("{} {:?}", request.request_line, request.headers).to_lowercase();
        assert!(!sent.contains("acme") && !sent.contains("tools"), "the private name was sent: {request:?}");
    }
    nothing_was_answered(&session);
}

// ---- 5. the setting is off ---------------------------------------------------------------------------------------

#[test]
fn with_lookups_off_every_row_says_so_and_nothing_is_requested() {
    let session = World::get().session("pc-off");
    let rig = PackageRig::start();
    rig.serve_npm("zinc-off-tool", 3, 12, true);
    rig.set_lookups(false);
    let (pane, card) = park(&session, "npm install zinc-off-tool");

    let view = rig.lookup(&pane, &prompt_id(&card), false).unwrap_or_else(|e| panic!("lookup failed: {e}"));
    assert_eq!(view["off"], true, "{view}");
    assert_eq!(view["statusText"], "Lookups are off", "{view}");
    assert_eq!(rows(&view).len(), 1, "{view}");
    assert_eq!(rows(&view)[0]["state"], "not_checked", "{view}");
    assert!(row_text(&rows(&view)[0]).contains("Not checked: package lookups are off"), "{view}");
    assert!(rig.requests().is_empty(), "a request was made with lookups off: {:?}", rig.requests());
    nothing_was_answered(&session);
}

// ---- 6. a prompt that is no longer the one on the screen --------------------------------------------------------------

#[test]
fn a_prompt_id_that_is_not_on_the_screen_is_stale_and_nothing_is_requested() {
    let session = World::get().session("pc-stale");
    let rig = PackageRig::start();
    rig.serve_npm("zinc-stale-tool", 3, 12, true);
    let (pane, card) = park(&session, "npm install zinc-stale-tool");

    assert_eq!(rig.lookup(&pane, "not-the-prompt-on-screen", false), Err("STALE_PROMPT".to_string()));
    assert_eq!(rig.lookup("no-such-pane", &prompt_id(&card), false), Err("STALE_PROMPT".to_string()));
    assert!(rig.requests().is_empty(), "a request was made for a stale prompt: {:?}", rig.requests());
    nothing_was_answered(&session);
}

// ---- 7. a secret typed as a package name ------------------------------------------------------------------------------

#[test]
fn a_secret_typed_as_a_package_name_is_counted_and_never_shown_or_looked_up() {
    const SECRET: &str = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
    let session = World::get().session("pc-secret");
    let rig = PackageRig::start();
    let (pane, card) = park(&session, &format!("npm i {SECRET}"));

    assert_eq!(card["packages"]["hidden"], 1, "{card}");
    assert_eq!(card["packages"]["items"], json!([]), "{card}");
    assert!(!card["packages"].to_string().contains(SECRET), "the secret is in the packages: {card}");

    let view = rig.lookup(&pane, &prompt_id(&card), false).unwrap_or_else(|e| panic!("lookup failed: {e}"));
    assert_eq!(view["hidden"], 1, "{view}");
    assert_eq!(rows(&view).len(), 0, "{view}");
    assert!(view["footer"].as_str().unwrap_or_default().contains("looked like a secret"), "{view}");
    assert!(!view.to_string().contains(SECRET), "the secret is in the view: {view}");
    assert!(rig.requests().is_empty(), "a request was made for a secret: {:?}", rig.requests());
    nothing_was_answered(&session);
}

// ---- 8. two ecosystems on one line --------------------------------------------------------------------------------------

#[test]
fn two_ecosystems_on_one_line_give_two_rows_each_from_its_own_registry() {
    let session = World::get().session("pc-two");
    let rig = PackageRig::start();
    // PyPI has nothing under this name (the fake answers 404); crates.io has a crate.
    rig.serve("/api/v1/crates/serde", 200, include_str!("../src/package_check/fixtures/crate.json"));
    let (pane, card) = park(&session, "pip install reqests && cargo add serde");

    let items = card["packages"]["items"].as_array().expect("packages on the card");
    let kinds: Vec<(&str, &str)> = items.iter().map(|i| (i["ecosystem"].as_str().unwrap_or(""), i["name"].as_str().unwrap_or(""))).collect();
    assert_eq!(kinds, [("pypi", "reqests"), ("crates", "serde")], "{card}");

    let view = rig.lookup(&pane, &prompt_id(&card), false).unwrap_or_else(|e| panic!("lookup failed: {e}"));
    assert_eq!(rows(&view).len(), 2, "{view}");
    assert_eq!((rows(&view)[0]["ecosystem"].as_str(), rows(&view)[0]["name"].as_str()), (Some("pypi"), Some("reqests")), "{view}");
    assert_eq!(rows(&view)[0]["state"], "not_found", "{view}");
    assert!(row_text(&rows(&view)[0]).contains("Not found on PyPI"), "{view}");
    assert_eq!((rows(&view)[1]["ecosystem"].as_str(), rows(&view)[1]["name"].as_str()), (Some("crates"), Some("serde")), "{view}");
    assert_eq!(rows(&view)[1]["state"], "checked", "{view}");

    let mut paths = rig.paths();
    paths.sort();
    assert_eq!(paths, ["/api/v1/crates/serde", "/simple/reqests/"], "{:?}", rig.requests());
    nothing_was_answered(&session);
}
