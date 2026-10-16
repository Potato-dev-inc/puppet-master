//! The Agent Wizard against a real ConPTY, for `tests/fake_tui_e2e.rs`: a wizard whose panes are the real
//! pane registry of the windowless test app, whose work folders live in the test's scratch folder, and
//! whose "unknown agent" is `pm-fake-tui` copied to `<scratch>/bin/<name>.exe` and found through `PATH`.
//!
//! Nothing here opens a window (the wizard's pane window option is off) and nothing touches the real
//! agents folder: finished definitions land in `<scratch>/agents`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::WizardEnv;
use crate::tui::declarative::wizard::visible::capture::Config;
use crate::tui::declarative::wizard::visible::runner::{RunnerCfg, TestLimits};
use crate::tui::declarative::wizard::{api, Wizard};

use super::world::Session;

/// A wizard over the test app. Restores `PATH` and `PM_FAKE_TUI_SCRIPT` when dropped.
pub struct WizardRig {
    wizard: Wizard,
    agents: PathBuf,
    saved_path: Option<std::ffi::OsString>,
    saved_script: Option<std::ffi::OsString>,
    /// The environment was not changed (the real-program rig): nothing to restore on drop.
    keep_env: bool,
}

impl Session {
    /// A wizard for the program `name` (a copy of `pm-fake-tui` named `<name>.exe`, put first on `PATH`).
    /// `script` is what the fake plays (see `tools/pm-fake-tui/README.md`).
    pub fn wizard_rig(&self, name: &str, script: &str) -> WizardRig {
        self.script(script);
        let bin = self.aux().join("bin");
        std::fs::create_dir_all(&bin).expect("bin folder");
        let exe = bin.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
        std::fs::copy(self.fake_exe(), &exe).expect("copy the fake agent");
        let saved_path = std::env::var_os("PATH");
        let mut dirs = vec![bin];
        dirs.extend(std::env::split_paths(&saved_path.clone().unwrap_or_default()));
        std::env::set_var("PATH", std::env::join_paths(dirs).expect("a PATH"));
        let saved_script = std::env::var_os("PM_FAKE_TUI_SCRIPT");
        std::env::set_var("PM_FAKE_TUI_SCRIPT", self.script_path());

        let agents = self.aux().join("agents");
        let temp = self.aux().join("wizard-temp");
        std::fs::create_dir_all(&temp).expect("wizard temp folder");
        let mut env = WizardEnv::app();
        env.agents_dir = agents.clone();
        env.temp_root = temp;
        env.path_dirs = None;
        env.app_build = "fake-tui-e2e".into();
        env.visible = RunnerCfg {
            capture: Config { stable_ms: 1200, frozen_ms: 3000, idle_done_ms: 3000, question_ms: 4000, ..Config::default() },
            // The fake is quick: a profile that never reads it as ready fails in seconds, not minutes.
            test: TestLimits { ready: Duration::from_secs(12), done: Duration::from_secs(60), permission: Duration::from_secs(30), ..TestLimits::default() },
            tick: Duration::from_millis(100),
            window: false,
            ..RunnerCfg::default()
        };
        WizardRig { wizard: Wizard::new(move || env.clone()), agents, saved_path, saved_script, keep_env: false }
    }
}

impl WizardRig {
    /// One wizard action; the error is the route's JSON error.
    pub fn call(&self, action: &str, body: Value) -> Result<Value, Value> {
        api::handle(&self.wizard, action, &body).map_err(|e| e.to_json())
    }

    pub fn agents_dir(&self) -> &Path {
        &self.agents
    }

    /// The session's random test words `(one, two, sky)`: a test may look, an AI may not.
    pub fn words(&self, session: &str) -> (String, String, String) {
        self.wizard.read(session, |s| (s.words.one.clone(), s.words.two.clone(), s.words.sky.clone())).expect("the session exists")
    }

    pub fn session_count(&self) -> usize {
        self.wizard.session_count()
    }

    /// "Check with fixtures" on a written definition: `{status, results: [{fixture, passed, failures}]}`.
    pub fn check_definition(&self, file: &Path) -> Value {
        let code_ids: Vec<String> = crate::tui::registry::registry().code_ids().into_iter().map(String::from).collect();
        let refs: Vec<&str> = code_ids.iter().map(String::as_str).collect();
        match crate::tui::declarative::check::check_report(file, None, &refs) {
            Ok(report) => json!({"status": report.status, "results": report.results}),
            Err(e) => json!({"error": e}),
        }
    }

    /// The transient profile ids the wizard may still hold (none after a clean run).
    pub fn is_registered(&self, agent_type: &str) -> bool {
        crate::tui::is_registered(agent_type)
    }
}

impl Drop for WizardRig {
    fn drop(&mut self) {
        if self.keep_env {
            return;
        }
        match self.saved_path.take() {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        match self.saved_script.take() {
            Some(script) => std::env::set_var("PM_FAKE_TUI_SCRIPT", script),
            None => std::env::remove_var("PM_FAKE_TUI_SCRIPT"),
        }
    }
}

// ---- the benchmark (SPEC 11.13): the visible path against a real program, from a script ---------------
//
// These helpers PLAY THE PERSON for a hands-off benchmark run: when the wizard says a question is the
// person's to answer, they type the keys the person gave in `PM_WIZARD_BENCH_ANSWERS` (for example
// `trust=1,permission=y`). That is a dev tool for the benchmark and for the zero-token tests; the product
// never does it (the capture has no way to send a key into a question). A question with no answer given
// stops the run and says which one is needed.

/// The keys the benchmark types for each kind of question: `trust`, `login`, `permission`, `question`.
/// `<enter>` stands for the Enter key. `ready=yes` presses "It is ready" when the wizard cannot tell.
#[derive(Debug, Clone, Default)]
pub struct Answers(std::collections::BTreeMap<String, String>);

impl Answers {
    pub fn parse(text: &str) -> Self {
        let mut map = std::collections::BTreeMap::new();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            if let Some((kind, keys)) = part.split_once('=') {
                map.insert(kind.trim().to_string(), keys.to_string());
            }
        }
        Self(map)
    }

    pub fn from_env() -> Self {
        Self::parse(&std::env::var("PM_WIZARD_BENCH_ANSWERS").unwrap_or_default())
    }

    pub fn get(&self, kind: &str) -> Option<&str> {
        self.0.get(kind).map(String::as_str)
    }
}

fn err_text(e: &Value) -> String {
    e["message"].as_str().map(str::to_string).unwrap_or_else(|| e.to_string())
}

/// The agent error of the round before `round` (a folder named `round-N`), if it had one.
fn previous_agent_error(round: &Path) -> Option<String> {
    let n: usize = round.file_name()?.to_str()?.strip_prefix("round-")?.parse().ok()?;
    let previous = round.parent()?.join(format!("round-{}", n.checked_sub(1)?)).join("agent_error.txt");
    std::fs::read_to_string(previous).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

impl WizardRig {
    /// A wizard for the real program on the real `PATH`: nothing is copied and no script is set. Finished
    /// definitions go to `<out>/agents`.
    pub fn real(out: &Path) -> WizardRig {
        let agents = out.join("agents");
        let temp = out.join("wizard-temp");
        std::fs::create_dir_all(&temp).expect("wizard temp folder");
        let mut env = WizardEnv::app();
        env.agents_dir = agents.clone();
        env.temp_root = temp;
        env.path_dirs = None;
        env.visible = RunnerCfg { window: false, ..RunnerCfg::default() };
        WizardRig { wizard: Wizard::new(move || env.clone()), agents, saved_path: None, saved_script: None, keep_env: true }
    }

    fn type_keys(&self, session: &Session, pane: &str, keys: &str) {
        if keys == "<enter>" {
            session.type_as_user(pane, "", true).expect("the bench types");
        } else {
            session.type_as_user(pane, keys, false).expect("the bench types");
        }
    }

    /// Run the guided capture on `command`, answering for the person, and write `<out>/captures.json`
    /// (`{command, words, raw, masked}`). The error says which answer is missing.
    pub fn bench_capture(&self, session: &Session, command: &str, out: &Path, answers: &Answers, args: &[String], seconds: u64) -> Result<Value, String> {
        std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
        let started = self.call("start", json!({"command": command})).map_err(|e| err_text(&e))?;
        let id = started["session"].as_str().ok_or("no session")?.to_string();
        let result = self.bench_capture_in(session, &id, command, out, answers, args, seconds);
        if let Err(reason) = &result {
            // Never lose the evidence of a stop: the last looks with the engine's verdict for each, and
            // whatever steps did finish.
            self.save_evidence(&id, command, out, reason);
        }
        let _ = self.call("cancel", json!({"session": id}));
        result
    }

    /// `captures.json` as the benchmark reads it: `{command, words, raw, masked}`.
    fn captures_dump(&self, id: &str, command: &str, finished: &Value) -> Result<Value, String> {
        let (one, two, sky) = self.words(id);
        let raw = self
            .wizard
            .read(id, |s| s.visible.captures.lock().clone().map(|set| set.to_json()))
            .map_err(|e| e.to_string())?
            .ok_or("nothing was captured")?;
        Ok(json!({
            "command": command,
            "words": {"one": one, "two": two, "sky": sky},
            "raw": raw,
            "masked": finished["captures"],
        }))
    }

    /// Write `<out>/capture-debug.json` (the last looks, real and masked, with the classifier's verdict
    /// and the engine's reading of each) and the partial `<out>/captures.json`.
    fn save_evidence(&self, id: &str, command: &str, out: &Path, reason: &str) {
        let run = self.wizard.read(id, |s| (s.visible.run.lock().clone(), s.visible.debug.lock().clone())).ok();
        if let Some((run, kept)) = run {
            let debug = run.map(|r| r.debug_json(true)).or(kept).unwrap_or(Value::Null);
            let doc = json!({"command": command, "reason": reason, "debug": debug});
            let _ = std::fs::write(out.join("capture-debug.json"), serde_json::to_string_pretty(&doc).unwrap_or_default());
        }
        if let Ok(finished) = self.call("capture", json!({"session": id, "op": "finish"})) {
            if let Ok(mut dump) = self.captures_dump(id, command, &finished) {
                dump["partial"] = json!(true);
                dump["stopped"] = json!(reason);
                let _ = std::fs::write(out.join("captures.json"), serde_json::to_string_pretty(&dump).unwrap_or_default());
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn bench_capture_in(&self, session: &Session, id: &str, command: &str, out: &Path, answers: &Answers, args: &[String], seconds: u64) -> Result<Value, String> {
        self.call("probe", json!({"session": id})).map_err(|e| err_text(&e))?;
        let plan = self.call("capture", json!({"session": id, "op": "plan", "args": args})).map_err(|e| err_text(&e))?;
        let fingerprint = plan["plan"]["fingerprint"].as_str().ok_or("no plan")?.to_string();
        let first = self
            .call("capture", json!({"session": id, "op": "start", "args": args, "confirm": true, "plan": fingerprint}))
            .map_err(|e| err_text(&e))?;
        let pane = first["pane"].as_str().ok_or("no pane")?.to_string();
        let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
        let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let s = self.call("capture", json!({"session": id, "op": "status"})).map_err(|e| err_text(&e))?;
            if let Some(kind) = s["needs_you"]["kind"].as_str() {
                let once = format!("{}-{kind}", s["step"].as_str().unwrap_or(""));
                if !done.contains(&once) {
                    match (kind, answers.get(kind)) {
                        ("unsure" | "unsure_reply", Some(_)) => {
                            self.call("capture", json!({"session": id, "op": "act", "action": "ready"})).map_err(|e| err_text(&e))?;
                        }
                        // `stuck=skip` / `stuck=retry`: the bench presses Skip or Retry for the person
                        ("stuck", Some(button @ ("skip" | "retry"))) => {
                            self.call("capture", json!({"session": id, "op": "act", "action": button})).map_err(|e| err_text(&e))?;
                        }
                        ("slow" | "typing" | "stuck", _) => {
                            return Err(format!(
                                "the capture needs you ({kind}): {} Give stuck=skip or stuck=retry in PM_WIZARD_BENCH_ANSWERS to go on.",
                                s["needs_you"]["message"].as_str().unwrap_or("")
                            ));
                        }
                        (_, Some(keys)) => self.type_keys(session, &pane, keys),
                        (_, None) => {
                            return Err(format!(
                                "the capture needs you: {} Give the keys to type with PM_WIZARD_BENCH_ANSWERS={kind}=<keys>",
                                s["needs_you"]["message"].as_str().unwrap_or("")
                            ));
                        }
                    }
                    done.insert(once);
                }
            }
            if s["step"] == "exit" && !done.contains("exit") {
                done.insert("exit".into());
                self.call("capture", json!({"session": id, "op": "act", "action": "try_exit"})).map_err(|e| err_text(&e))?;
            }
            if s["settled"] == true {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!("the capture did not settle in {seconds} s: {}", s["steps"]));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let finished = self.call("capture", json!({"session": id, "op": "finish"})).map_err(|e| err_text(&e))?;
        let dump = self.captures_dump(id, command, &finished)?;
        std::fs::write(out.join("captures.json"), serde_json::to_string_pretty(&dump).unwrap_or_default()).map_err(|e| e.to_string())?;
        Ok(dump)
    }

    /// One round of the visible benchmark: verify `draft` against the captured screens in `dump`, run
    /// the visible test on the real program (answering for the person), and write `verify.txt`,
    /// `visible.json`, `feedback.json`, `agent_error.txt` and `result.txt` into `round`. Returns the
    /// result line: `PASS`, `FAIL verify`, `FAIL visibletest` or `STOP agent-error`.
    #[allow(clippy::too_many_arguments)]
    pub fn bench_round(
        &self,
        session: &Session,
        command: &str,
        round: &Path,
        draft: &str,
        expects: &std::collections::BTreeMap<String, String>,
        dump: &Value,
        answers: &Answers,
        finish: bool,
    ) -> Result<String, String> {
        let write = |name: &str, text: &str| std::fs::write(round.join(name), text).map_err(|e| format!("{name}: {e}"));
        let started = self.call("start", json!({"command": command})).map_err(|e| err_text(&e))?;
        let id = started["session"].as_str().ok_or("no session")?.to_string();
        let result = self.bench_round_in(session, &id, command, round, draft, expects, dump, answers, finish, &write);
        if result.is_err() {
            let _ = self.call("cancel", json!({"session": id}));
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn bench_round_in(
        &self,
        session: &Session,
        id: &str,
        command: &str,
        round: &Path,
        draft: &str,
        expects: &std::collections::BTreeMap<String, String>,
        dump: &Value,
        answers: &Answers,
        finish: bool,
        write: &dyn Fn(&str, &str) -> Result<(), String>,
    ) -> Result<String, String> {
        self.call("probe", json!({"session": id})).map_err(|e| err_text(&e))?;
        let words = &dump["words"];
        let set = crate::tui::declarative::wizard::visible::capture::CaptureSet::from_json(&dump["raw"])?;
        let session_words = crate::tui::declarative::wizard::session::TestWords {
            one: words["one"].as_str().ok_or("the dump has no words")?.to_string(),
            two: words["two"].as_str().ok_or("the dump has no words")?.to_string(),
            sky: words["sky"].as_str().ok_or("the dump has no words")?.to_string(),
        };
        self.wizard.bench_restore(id, session_words, set).map_err(|e| e.to_string())?;

        let verify = self.call("verify", json!({"session": id, "toml": draft, "expect": expects})).map_err(|e| err_text(&e))?;
        let (result, text, visible) = if verify["ok"] != true && verify.get("planned_visible").is_none_or(Value::is_null) {
            ("FAIL verify", verify["text"].as_str().unwrap_or("").to_string(), Value::Null)
        } else {
            let plan = verify["planned_visible"]["fingerprint"].as_str().ok_or("verify gave no plan")?.to_string();
            let known: std::collections::HashSet<String> = session.pane_ids().into_iter().collect();
            let visible = std::thread::scope(|scope| {
                let handle = scope.spawn(|| self.call("visibletest", json!({"session": id, "confirm": true, "plan": plan})));
                let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
                while !handle.is_finished() {
                    if let Ok(status) = self.call("capture", json!({"session": id, "op": "test_status"})) {
                        if let Some(kind) = status["needs_you"]["kind"].as_str() {
                            let once = format!("{}-{}", status["check"].as_str().unwrap_or(""), kind);
                            if !done.contains(&once) {
                                if let (Some(keys), Some(pane)) = (answers.get(kind), session.pane_ids().into_iter().find(|p| !known.contains(p))) {
                                    self.type_keys(session, &pane, keys);
                                    done.insert(once);
                                }
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                handle.join().unwrap_or_else(|_| Err(json!({"message": "the visible test crashed"})))
            })
            .map_err(|e| err_text(&e))?;
            let agent_error = visible["agent_error"]["message"].as_str().map(str::to_string);
            if let Some(message) = &agent_error {
                write("agent_error.txt", message)?;
            }
            let repeated = agent_error.as_deref().is_some_and(|m| previous_agent_error(round).as_deref() == Some(m));
            if visible["ok"] == true {
                ("PASS", String::new(), visible)
            } else if visible["agent_error"]["external"] == true || repeated {
                let message = agent_error.unwrap_or_default();
                (
                    "STOP agent-error",
                    format!("{command} itself returned an error: {message}. This is a problem with the setup of {command} (sign-in, model or provider), not with the profile. Fix it in {command}, then run this round again."),
                    visible,
                )
            } else {
                ("FAIL visibletest", visible["text"].as_str().unwrap_or("").to_string(), visible)
            }
        };
        write("verify.txt", &format!("{}\n{}", if verify["ok"] == true { "OK" } else { "NOT OK" }, verify["text"].as_str().unwrap_or("")))?;
        write("visible.json", &serde_json::to_string_pretty(&visible).unwrap_or_default())?;
        write(
            "feedback.json",
            &serde_json::to_string_pretty(&json!({"previous_toml": draft, "previous_expects": expects, "text": text, "samples": []})).unwrap_or_default(),
        )?;
        write("result.txt", &format!("{result}\n"))?;
        if finish && result == "PASS" {
            let done = self
                .call("finish", json!({"session": id, "provenance": {"drafter": "Benchmark", "drafter_kind": "other", "prompt_version": "5", "rounds": 1}}))
                .map_err(|e| err_text(&e))?;
            write("finished.json", &serde_json::to_string_pretty(&done).unwrap_or_default())?;
        } else {
            let _ = self.call("cancel", json!({"session": id}));
        }
        Ok(result.to_string())
    }
}

impl WizardRig {
    /// Ask the app's wizard host to press Enter in `pane`, as the wizard's answerer would. It must refuse any
    /// pane the wizard did not start (the person's own panes, an agent run's).
    pub fn wizard_host_presses_enter(&self, pane: &str) -> Result<(), String> {
        use crate::tui::declarative::wizard::visible::answer::AnswerKey;
        use crate::tui::declarative::wizard::visible::host::{LiveHost, PaneHost};
        LiveHost.answer(pane, &[AnswerKey::Enter])
    }
}
