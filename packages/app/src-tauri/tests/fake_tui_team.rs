//! Co-work Heads-up, end to end, with two people (COWORK-PLAN.md Story 1): Maya's agent pushes, Maya
//! tells the team, Leo gets a card, Leo queues his agent, the conflict is resolved in a separate copy,
//! Leo applies it. No real people, no GitHub, no real agents.
//!
//! What is real: two Puppet Master processes (this test and a second run of this binary, each an
//! isolated world with its own app-data folder, panes and in-memory key store; see
//! `src/fake_tui/team.rs`), the Team service in each (invite, join, approval, signed roster, sealed
//! events, the queue runner), a real relay on a loopback port (`pm-relay`), a real git remote (a
//! bare repository on this disk with a hook that logs every push) and two real clones, and fake
//! agents (`pm-fake-tui`) on real ConPTY panes that write files and run git. Every click is one
//! call of the function the cockpit's command calls.
//!
//! Included from `fake_tui_e2e.rs` (one test binary), so the fake-agent shards of CI run it.

#![cfg(all(windows, feature = "dev-fake-tui"))]

use puppet_master_app_lib::fake_tui::{leak_check, Person, Remote};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BASE: &str = "export function session(user) {\n  return { user, ttl: 3600 };\n}\n\nexport function login(user) {\n  return session(user);\n}\n";
const MAYA_LINE: &str = "  return refreshable(session(user));";
const LEO_LINE: &str = "  return session(user, { remember: true });";
const RESOLVED_LINE: &str = "  return refreshable(session(user, { remember: true }));";
const MAYA_COMMIT_MESSAGE: &str = "Add OAuth token refresh (ticket PM-1234)";
const NOTE: &str = "Touches login(), careful with your billing branch";

fn with_line(line: &str) -> String {
    BASE.replace("  return session(user);", line)
}

/// A TOML basic string.
fn toml_str(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().expect("git");
    assert!(out.status.success(), "git {args:?} in {}: {}", dir.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git").args(args).current_dir(dir).output().map(|o| o.status.success()).unwrap_or(false)
}

fn slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// The path as the app compares paths: resolved (an 8.3 short name such as `RUNNER~1`, or a junction,
/// becomes the real folder, which is how git and the app spell it), without `\?\`, lowercase.
/// A folder can be spelled several ways on Windows; a test must not depend on which one it was given.
fn canonical(path: &Path) -> String {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    slashes(&real).trim_start_matches("//?/").to_string()
}

/// Polls `check` until it gives something, or panics with `what` after `secs`. The only waiting in
/// this test: always on a condition, never for a fixed time.
fn until<T>(secs: u64, what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "gave up waiting {secs} s for: {what}");
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Both people behind one interface, so a step reads the same for either.
enum Who<'a> {
    Here(&'a Person),
    There(&'a mut Remote),
}

impl Who<'_> {
    fn call(&mut self, op: &str, args: Value) -> Value {
        let reply = match self {
            Who::Here(p) => p.call(op, &args),
            Who::There(r) => r.call(op, args),
        };
        reply.unwrap_or_else(|e| panic!("{op} failed: {e}"))
    }

    fn view(&mut self) -> Value {
        self.call("view", json!({}))
    }

    fn config(&mut self) -> Value {
        self.call("config", json!({}))
    }

    fn cards(&mut self) -> Vec<Value> {
        self.view()["cards"].as_array().cloned().unwrap_or_default()
    }

    fn job(&mut self, id: &str) -> Value {
        self.view()["jobs"].as_array().and_then(|jobs| jobs.iter().find(|j| j["id"] == id).cloned()).unwrap_or(Value::Null)
    }
}

/// A bare "origin" that logs the ref of every push it receives (`push.log`).
struct Origin {
    origin: PathBuf,
}

impl Origin {
    fn pushes(&self) -> Vec<String> {
        std::fs::read_to_string(self.origin.join("push.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn refs(&self) -> String {
        git(&self.origin, &["for-each-ref", "--format=%(refname) %(objectname)"])
    }
}

fn clone_of(origin: &Path, dir: &Path, name: &str) {
    git(dir.parent().unwrap(), &["clone", "-q", &origin.to_string_lossy(), dir.file_name().unwrap().to_str().unwrap()]);
    git(dir, &["config", "user.name", name]);
    git(dir, &["config", "user.email", &format!("{}@example.invalid", name.to_lowercase())]);
}

#[test]
fn two_people_tell_each_other_through_a_relay_and_nothing_lands_without_a_click() {
    // The same binary is also Leo's process: he serves Maya's calls until she hangs up.
    if Remote::serve_if_child() {
        return;
    }
    let started = Instant::now();
    let lap = |what: &str| eprintln!("[team e2e {:>5.1}s] {what}", started.elapsed().as_secs_f32());

    // Leo's process starts while this one sets up.
    let mut leo_remote = Remote::spawn("Leo", module_path!(), "two_people_tell_each_other_through_a_relay_and_nothing_lands_without_a_click");
    let maya_person = Person::start("Maya");
    let mut maya = Who::Here(&maya_person);
    let mut leo = Who::There(&mut leo_remote);
    lap("both worlds started");

    // ---- 1. Setup: a git origin, two clones that edit the same line, a relay -----------------------
    // Not under `aux`: Windows reserves that name for git.
    let work = maya_person.aux().parent().unwrap().join("repos");
    std::fs::create_dir_all(&work).unwrap();
    let origin_dir = work.join("origin.git");
    git(&work, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    let seed = work.join("seed");
    clone_of(&origin_dir, &seed, "Seed");
    git(&seed, &["checkout", "-q", "-b", "main"]);
    std::fs::create_dir_all(seed.join("src")).unwrap();
    std::fs::write(seed.join("src/auth.ts"), BASE).unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-q", "-m", "Start"]);
    git(&seed, &["push", "-q", "-u", "origin", "main"]);
    // From here on every push the origin receives is written down.
    std::fs::write(origin_dir.join("hooks").join("pre-receive"), "#!/bin/sh\nwhile read old new ref; do echo \"$ref\" >> push.log; done\n").unwrap();
    let origin = Origin { origin: origin_dir.clone() };
    let maya_clone = work.join("maya");
    let leo_clone = work.join("leo");
    clone_of(&origin_dir, &maya_clone, "Maya");
    clone_of(&origin_dir, &leo_clone, "Leo");
    // Leo has his own commit on the same line, on his own branch, not pushed.
    git(&leo_clone, &["checkout", "-q", "-b", "leo/billing-page"]);
    std::fs::write(leo_clone.join("src/auth.ts"), with_line(LEO_LINE)).unwrap();
    git(&leo_clone, &["commit", "-q", "-am", "Remember me on login"]);
    let leo_head = git(&leo_clone, &["rev-parse", "HEAD"]);
    git(&maya_clone, &["checkout", "-q", "-b", "feat/oauth-refresh"]);
    let maya_root = git(&maya_clone, &["rev-parse", "HEAD"]);
    assert_eq!(origin.pushes(), Vec::<String>::new());

    let (relay_addr, relay) = pm_relay::spawn_local(pm_relay::Config::default());
    let relay_url = format!("http://{relay_addr}");
    assert!(relay_url.starts_with("http://127.0.0.1:"), "the relay is on this computer");
    lap("repositories and relay ready");

    // ---- 2. Maya creates the team, invites Leo, Leo joins, Maya approves ------------------------------
    for (who, clone) in [(&mut maya, &maya_clone), (&mut leo, &leo_clone)] {
        let config = who.call("enable", json!({ "projects": [clone.to_string_lossy()] }));
        assert_eq!(config["enabled"], true);
        assert_eq!(config["keyStore"], "memory", "keys are kept in memory here, never in the OS credential store");
    }
    let config = maya.call("create", json!({ "team_name": "Acme", "name": "Maya", "relay": relay_url }));
    assert_eq!(config["admin"], true);
    assert_eq!(config["members"].as_array().unwrap().len(), 1);
    let team = config["team"].as_str().unwrap().to_string();
    let invite = maya.call("invite", json!({}));
    let link = invite["link"].as_str().unwrap().to_string();
    let code = invite["code"].as_str().unwrap().to_string();
    assert!(!link.contains(&code) && !link.contains(&code.replace('-', "")), "the code is not in the link");

    let joining = leo.call("join", json!({ "link": link, "code": code, "name": "Leo" }));
    assert_eq!(joining["joining"], "Acme", "Leo waits for the admin's yes");
    assert!(leo.config()["members"].as_array().unwrap().is_empty(), "not on the team yet");

    let request = until(40, "Maya sees Leo's request", || maya.config()["joins"].as_array().and_then(|j| j.first().cloned()));
    assert_eq!(request["name"], "Leo");
    assert!(request["warning"].is_null(), "one person used the code");
    let after = maya.call("approve", json!({ "join_id": request["id"] }));
    assert_eq!(after["members"].as_array().unwrap().len(), 2);
    let leo_config = until(40, "Leo is let in", || {
        let c = leo.config();
        (c["joining"].is_null() && c["members"].as_array().is_some_and(|m| m.len() == 2)).then_some(c)
    });
    assert_eq!(leo_config["admin"], false);
    assert_eq!(leo_config["team"].as_str().unwrap(), team);
    let leo_id = after["members"].as_array().unwrap().iter().find(|m| m["name"] == "Leo").unwrap()["id"].as_str().unwrap().to_string();
    until(30, "both apps are live on the relay", || {
        (maya.config()["link"] == "live" && leo.config()["link"] == "live").then_some(())
    });
    lap("team formed (create, invite, join, approve)");

    // ---- 3. Maya's agent commits and pushes; Maya gets "Tell the team?"; she says yes -----------------
    for (who, clone) in [(&mut maya, &maya_clone), (&mut leo, &leo_clone)] {
        let want = canonical(clone);
        // A repository is watched once the app has taken its first look (the baseline); a runner
        // with two slow cores gets a generous deadline for that first look.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let seen: Vec<String> = who.call("watched", json!({})).as_array().unwrap().iter().map(|d| canonical(Path::new(d.as_str().unwrap()))).collect();
            if seen.contains(&want) {
                break;
            }
            assert!(Instant::now() < deadline, "gave up waiting 60 s for the app to watch {want} for pushes; it watches {seen:?}");
            std::thread::sleep(Duration::from_millis(40));
        }
    }
    // Leo has Claude Code open in his clone, idle.
    let leo_pane = leo.call("agent_pane", json!({ "agent_type": "fake_claude", "cwd": leo_clone.to_string_lossy() })).as_str().unwrap().to_string();
    until(30, "Leo's agent is idle", || (leo.call("pane_status", json!({ "pane_id": leo_pane })) == "idle").then_some(()));

    let push_command = format!("git add -A && git commit -q -m \"{MAYA_COMMIT_MESSAGE}\" && git push -q -u origin feat/oauth-refresh");
    maya.call(
        "script",
        json!({ "toml": format!(
            "[[turn]]\non_prompt = \"oauth\"\nsteps = [\n  {{ do = \"think\", ms = 300 }},\n  {{ do = \"write_file\", path = \"src/auth.ts\", content = {} }},\n  {{ do = \"run\", command = {} }},\n  {{ do = \"say\", text = \"Pushed feat/oauth-refresh.\" }},\n]\n",
            toml_str(&with_line(MAYA_LINE)),
            toml_str(&push_command)
        ) }),
    );
    let run = maya.call("run", json!({ "agent_type": "fake_claude", "project": maya_clone.to_string_lossy(), "task": "Add OAuth token refresh to login()" }));
    assert_eq!(run["status"], "completed", "Maya's agent finished: {run}");
    let maya_head = git(&maya_clone, &["rev-parse", "HEAD"]);
    assert_ne!(maya_head, maya_root, "her agent committed");
    assert_eq!(git(&origin_dir, &["rev-parse", "refs/heads/feat/oauth-refresh"]), maya_head, "and pushed to origin");
    assert_eq!(origin.pushes(), ["refs/heads/feat/oauth-refresh"], "one push, by her agent");

    let draft = until(30, "Maya is asked: Tell the team?", || maya.view()["drafts"].as_array().and_then(|d| d.first().cloned()));
    assert_eq!(draft["event"]["kind"], "pushed");
    assert_eq!(draft["event"]["subject"]["branch"], "feat/oauth-refresh");
    assert!(!draft["event"]["actor"]["agent"].is_null(), "attributed to the agent that pushed: {draft}");
    assert!(leo.cards().is_empty(), "nothing is sent until she says yes");
    let draft_id = draft["id"].as_str().unwrap().to_string();
    maya.call("send", json!({ "draft_id": draft_id, "note": NOTE }));
    assert!(maya.view()["drafts"].as_array().unwrap().is_empty(), "the question is answered");
    lap("Maya's agent pushed, Maya told the team");

    // ---- 4. Leo's Inbox: the Team card -----------------------------------------------------------------
    let card = until(30, "Leo's card arrives", || leo.cards().into_iter().next());
    let event = &card["event"];
    assert_eq!(card["state"], "new");
    assert_eq!(card["id"], draft_id.as_str(), "the card is the event Maya sent");
    assert_eq!(event["kind"], "pushed");
    assert_eq!(event["actor"]["display"], "Maya");
    assert_eq!(event["project"]["name"], "origin");
    assert_eq!(event["subject"]["branch"], "feat/oauth-refresh");
    assert_eq!(event["subject"]["commit"], maya_head.as_str());
    assert_eq!(event["subject"]["paths"], json!(["src/auth.ts"]));
    assert_eq!(event["subject"]["stats"]["files"], 1);
    assert_eq!(event["note"], NOTE);
    // Metadata only: no diff, no file contents, no commit message, no path of Maya's computer, no
    // email, no prompt.
    let shown = card.to_string();
    for private in ["refreshable", "remember", "OAuth token refresh", "PM-1234", "example.invalid", &slashes(&maya_clone), &canonical(&maya_clone), "Add OAuth token refresh to login"] {
        assert!(!shown.to_lowercase().contains(&private.to_lowercase()), "the card must not carry {private:?}: {shown}");
    }
    assert_eq!(card["local"]["currentBranch"], "leo/billing-page");
    assert_eq!(card["local"]["canQueue"], true);
    assert_eq!(card["local"]["canPull"], false, "he is not on her branch");
    assert_eq!(card["panes"].as_array().unwrap().len(), 1, "his agent is offered");
    assert!(maya.cards().is_empty(), "Maya gets no card about her own push");
    assert_eq!(git(&leo_clone, &["rev-parse", "HEAD"]), leo_head, "a card changes nothing in his repository");
    lap("Leo has the Team card");

    // ---- 5. Leo: Queue my agent -> separate copy -> conflict resolved -> result card -> Apply ----------
    let refs_before = origin.refs();
    let pushes_before = origin.pushes();
    leo.call(
        "script",
        json!({ "toml": format!(
            "paste_burst_ms = 100\n[[turn]]\non_prompt = \"conflicted file\"\nsteps = [\n  {{ do = \"think\", ms = 300 }},\n  {{ do = \"write_file\", path = \"src/auth.ts\", content = {} }},\n  {{ do = \"say\", text = \"Resolved src/auth.ts.\" }},\n]\n",
            toml_str(&with_line(RESOLVED_LINE))
        ) }),
    );
    let job = leo.call("queue", json!({ "card_id": card["id"], "pane_id": leo_pane, "when": "idle" }));
    let job_id = job["id"].as_str().unwrap().to_string();
    let parked = until(40, "the merge stops on the conflict", || {
        let j = leo.job(&job_id);
        (j["state"] == "needs_start").then_some(j)
    });
    assert_eq!(parked["conflicts"], json!(["src/auth.ts"]));
    assert!(parked["copyBranch"].as_str().unwrap().starts_with("pm/"), "the work is in a separate copy");
    assert_eq!(git(&leo_clone, &["rev-parse", "HEAD"]), leo_head, "his branch has not moved");
    assert!(git(&leo_clone, &["status", "--porcelain"]).lines().all(|l| l.contains(".puppet-master")), "and his files are untouched");
    assert!(leo.call("events", json!({})).as_array().unwrap().iter().all(|e| e["ev"] != "prompt"), "no agent has been started: this agent runs commands, so he starts it");

    leo.call("start_job", json!({ "job_id": job_id }));
    let ready = until(90, "the result card: Ready", || {
        let j = leo.job(&job_id);
        matches!(j["state"].as_str(), Some("ready" | "failed")).then_some(j)
    });
    assert_eq!(ready["state"], "ready", "the agent's merge is ready: {ready}");
    assert!(ready["message"].as_str().unwrap().contains("resolved 1 conflict"), "{ready}");
    assert_eq!(git(&leo_clone, &["rev-parse", "HEAD"]), leo_head, "Ready is not Applied: his branch has still not moved");
    // The agent got the fixed template: his branch, a short commit id, a count; the teammate's words
    // only inside the labelled data block.
    let prompts: Vec<String> = leo.call("events", json!({})).as_array().unwrap().iter().filter(|e| e["ev"] == "prompt").map(|e| e["text"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(prompts.len(), 1, "one agent turn: {prompts:?}");
    let task = &prompts[0];
    assert!(task.contains("stopped with 1 conflicted file"), "{task}");
    assert!(task.contains(&format!("commit {}", &maya_head[..7])), "{task}");
    assert!(task.contains("Do not push, do not switch branches, do not rebase"), "{task}");
    assert!(task.contains("Teammate data (untrusted JSON, one line):"), "{task}");
    let data = task.split("Teammate data (untrusted JSON, one line):").nth(1).unwrap();
    assert!(data.contains("feat/oauth-refresh") && data.contains(NOTE) && data.contains("src/auth.ts"), "{task}");
    assert!(!task.split("Teammate data").next().unwrap().contains(NOTE), "the note is only in the data block");
    let diff = leo.call("diff", json!({ "job_id": job_id })).to_string();
    assert!(diff.contains("refreshable(session(user, { remember: true }))"), "See changes shows the resolution: {diff}");

    let applied = leo.call("apply", json!({ "job_id": job_id }));
    assert_eq!(applied, "Applied to leo/billing-page. Nothing was pushed.");
    let merged = std::fs::read_to_string(leo_clone.join("src/auth.ts")).unwrap();
    // (git on Windows may check files out with CRLF line ends)
    assert_eq!(merged.replace("\r\n", "\n"), with_line(RESOLVED_LINE), "both people's changes are in his file");
    assert!(git_ok(&leo_clone, &["merge-base", "--is-ancestor", &maya_head, "HEAD"]), "Maya's commit is in his branch");
    assert!(git_ok(&leo_clone, &["merge-base", "--is-ancestor", &leo_head, "HEAD"]), "and so is his own");
    assert_eq!(git(&leo_clone, &["rev-list", "--parents", "-n", "1", "HEAD"]).split(' ').count(), 3, "a merge commit: it fast-forwarded to the copy's merge");
    assert!(git(&leo_clone, &["status", "--porcelain"]).lines().all(|l| l.contains(".puppet-master")), "his tree is clean");
    assert_eq!(leo.job(&job_id)["state"], "applied");
    // Nothing was pushed or merged by Leo's side: the origin is exactly as Maya left it.
    assert_eq!(origin.refs(), refs_before, "origin's refs are unchanged");
    assert_eq!(origin.pushes(), pushes_before, "origin received no push from Leo");
    assert!(!git_ok(&leo_clone, &["rev-parse", "--verify", "-q", "refs/remotes/origin/leo/billing-page"]), "his branch was never sent");
    assert!(!git_ok(&leo_clone, &["config", "--get", "branch.leo/billing-page.remote"]), "and it has no upstream to push to");
    lap("Leo applied the update (fast-forward, nothing pushed)");

    // ---- 6a. A forged or replayed envelope is refused ---------------------------------------------------
    let genuine = maya.call("envelope", json!({ "relay": relay_url, "team": team, "id": draft_id }));
    let key = event["project"]["key"].as_str().unwrap().to_string();
    let cards_now = leo.cards();
    for (kind, why) in [
        ("tampered", "bad signature"),
        ("unsigned", "bad signature"),
        ("stranger", "sender is not on the team"),
        ("impersonator", "bad signature"),
    ] {
        let forged = maya.call("forge", json!({ "kind": kind, "envelope": genuine, "project_key": key }));
        assert_eq!(leo.call("inject", json!({ "envelope": forged })), why, "{kind}");
        assert_eq!(leo.cards(), cards_now, "{kind}: no card, nothing changed");
    }
    // The relay does not take a stranger's envelope either.
    let forged = maya.call("forge", json!({ "kind": "stranger", "envelope": genuine, "project_key": key }));
    let posted = maya.call("stranger_posts", json!({ "relay": relay_url, "envelope": forged }));
    assert_eq!(posted["accepted"], false, "{posted}");
    assert_eq!(posted["denied"], true, "{posted}");
    // The real envelope again: already seen, still one card.
    assert_eq!(leo.call("inject", json!({ "envelope": genuine })), "already seen");
    assert_eq!(leo.cards(), cards_now, "a replay is not a second card");
    lap("forged and replayed envelopes refused");

    // ---- 6b. A removed member gets nothing new and cannot read it ---------------------------------------
    let removed = maya.call("remove", json!({ "member_id": leo_id }));
    assert_eq!(removed["members"].as_array().unwrap().len(), 1);
    assert_eq!(removed["epoch"], 2, "the team key was replaced");
    let told = until(20, "Leo's app tells him he is out", || {
        let c = leo.config();
        c["error"].as_str().is_some_and(|e| e.contains("no longer on this team")).then_some(c)
    });
    assert!(told["link"].is_null(), "his app stopped listening: {told}");

    let second = "git checkout -q -b feat/after-removal && git add -A && git commit -q -m after && git push -q -u origin feat/after-removal";
    maya.call(
        "script",
        json!({ "toml": format!(
            "[[turn]]\non_prompt = \"second\"\nsteps = [\n  {{ do = \"write_file\", path = \"docs/notes.txt\", content = \"notes\\n\" }},\n  {{ do = \"run\", command = {} }},\n]\n",
            toml_str(second)
        ) }),
    );
    let run = maya.call("run", json!({ "agent_type": "fake_claude", "project": maya_clone.to_string_lossy(), "task": "second change" }));
    assert_eq!(run["status"], "completed", "{run}");
    let draft = until(30, "Maya is asked again", || maya.view()["drafts"].as_array().and_then(|d| d.first().cloned()));
    assert_eq!(draft["event"]["subject"]["branch"], "feat/after-removal");
    let after_id = draft["id"].as_str().unwrap().to_string();
    maya.call("send", json!({ "draft_id": after_id, "note": "only for the people still here" }));
    let sealed_after = maya.call("envelope", json!({ "relay": relay_url, "team": team, "id": after_id }));
    assert_eq!(sealed_after["epoch"], 2, "sealed with the new team key");
    // Even if the envelope reaches him some other way, his app cannot open it.
    assert_eq!(leo.call("inject", json!({ "envelope": sealed_after })), "no key for that epoch");
    assert_eq!(leo.cards(), cards_now, "no card for anything after the removal");
    lap("removed member locked out");

    // ---- 6c. The relay holds nothing readable ------------------------------------------------------------
    let dump = relay.dump();
    assert_eq!(leak_check(&dump, &[&team]), [team.as_str()], "the check does read the relay's bodies (the team id is routing data)");
    let shown_code = code.clone();
    let plain_code = code.replace('-', "");
    let private = [
        "Maya", "Leo", "Acme", "feat/oauth-refresh", "oauth-refresh", "feat/after-removal", "src/auth.ts", "refreshable",
        "Touches login", "billing", "PM-1234", "remember", "only for the people", "fake_claude", "Claude Code", "origin",
        shown_code.as_str(), plain_code.as_str(), maya_head.as_str(),
    ];
    assert_eq!(leak_check(&dump, &private), Vec::<&str>::new(), "the relay must not hold these");

    drop(maya);
    drop(leo);
    leo_remote.quit();
    lap("done");
}
