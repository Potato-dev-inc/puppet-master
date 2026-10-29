//! The live entry of the pick: reads the person's own Scores rows of the folder (last 90 days), the
//! agent registry, the saved logins and the limits the app has seen, then calls `compose::pick`.
//!
//! Everything it touches sits behind [`PickIo`], so `pick_for_in` runs against a fake in the tests.
//! Read-only: nothing is written, logged or sent. The only input is the folder path; there is no
//! text of the person here, and nothing leaves the computer.
//!
//! Generic: agents are only ids and registry labels (wizard and Store agents included).

use super::compose;
use super::login::{self, RowFact};
use super::model::{Cand, Pick, Usual, PICK_DAYS};
use crate::handoff::keep_going::live_io::label_with;
use crate::handoff::keep_going::model::Target;
use crate::scores::model::ScoreRow;

/// The longest folder path the pick accepts.
const MAX_FOLDER_CHARS: usize = 1024;
/// The fourth note: shown only when the history could not be read.
const UNREADABLE_NOTE: &str = "Your history could not be read, so this is your usual agent.";

/// One registered agent, as the pick needs to know it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentFact {
    pub id: String,
    pub label: String,
    pub installed: bool,
    /// Can be started by a run.
    pub dispatchable: bool,
    /// A command line agent (not a window or a service).
    pub cli: bool,
    /// The billing vendor from the agent's limits table, as written (may be empty or `varies`).
    pub vendor: Option<String>,
}

/// Everything the pick reads from the running app.
pub trait PickIo {
    /// The registered agents, in registry order.
    fn agents(&self) -> Vec<AgentFact>;
    /// Whether the agent could start in `folder`, in plain words when not.
    fn runnable(&self, agent: &AgentFact, folder: &str) -> Result<(), String>;
    /// Whether the saved login exists for the agent and may run.
    fn account_ok(&self, agent_id: &str, account_id: &str) -> bool;
    /// The saved login's label, when it still exists.
    fn account_label(&self, account_id: &str) -> Option<String>;
    /// The limit seen for this agent and login: `Some(Some(ms))` out until then, `Some(None)` out
    /// with no time known.
    fn limit(&self, agent_id: &str, account: Option<&str>) -> Option<Option<u64>>;
    /// The folder's Scores rows of the last `PICK_DAYS` days.
    fn rows(&self, folder: &str) -> Result<Vec<ScoreRow>, String>;
    /// The person's usual agents (the Ask bar default and the last one used).
    fn usual(&self) -> Usual;
    /// A reset time as the person reads it ("17:00").
    fn when(&self, ms: u64) -> String;
}

/// A vendor that really names one billing company. `None`, empty and `varies` do not, so an agent
/// without a known company never shares a plan with another.
fn known_vendor(vendor: Option<&str>) -> Option<String> {
    vendor.filter(|v| !v.is_empty() && *v != "varies").map(str::to_string)
}

/// The answer for `folder`: one candidate per agent, with the login the person uses here.
pub fn pick_for_in(io: &dyn PickIo, folder: &str) -> Pick {
    let (rows, readable) = match io.rows(folder) {
        Ok(rows) => (rows, true),
        // Unreadable history: no evidence for anyone, so the pick falls back to the usual agent.
        Err(_) => (Vec::new(), false),
    };
    let cands: Vec<Cand> = io
        .agents()
        .into_iter()
        .filter(|agent| agent.dispatchable && agent.cli)
        .map(|agent| candidate(io, &agent, &rows, folder))
        .collect();
    let mut answer = compose::pick(&cands, &io.usual(), &|ms| io.when(ms));
    if !readable {
        answer.notes.push(UNREADABLE_NOTE.to_string());
    }
    answer
}

/// One agent, with its usual login here, that login's limit and that login's own row.
fn candidate(io: &dyn PickIo, agent: &AgentFact, rows: &[ScoreRow], folder: &str) -> Cand {
    let mine: Vec<&ScoreRow> = rows.iter().filter(|row| row.agent_id == agent.id).collect();
    let facts: Vec<RowFact> =
        mine.iter().map(|row| RowFact { account_id: row.account_id.clone(), turns: row.turns }).collect();
    let account = login::usual_login(&facts, &|id| io.account_ok(&agent.id, id));
    let evidence = mine.iter().find(|row| row.account_id == account).map(|row| login::evidence_of(row));

    let mut ready = io.runnable(agent, folder);
    if ready.is_ok() {
        if let Some(id) = account.as_deref() {
            if !io.account_ok(&agent.id, id) {
                ready = Err("That saved login isn't available any more".to_string());
            }
        }
    }
    let account_label = account.as_deref().and_then(|id| io.account_label(id));
    Cand {
        target: Target { agent_id: agent.id.clone(), account: account.clone() },
        label: label_with(&agent.label, account_label.as_deref()),
        vendor: known_vendor(agent.vendor.as_deref()),
        ready,
        // The limit of THIS login only: it never moves the pick to another login.
        limited_until: io.limit(&agent.id, account.as_deref()),
        evidence,
    }
}

/// The folder path as the command accepts it: trimmed, 1 to 1024 characters, no control character.
pub fn check_folder(project: &str) -> Result<String, String> {
    let folder = project.trim();
    let count = folder.chars().count();
    if count == 0 || count > MAX_FOLDER_CHARS {
        return Err(format!("INVALID_ARGUMENT: project must be 1 to {MAX_FOLDER_CHARS} characters"));
    }
    if folder.chars().any(char::is_control) {
        return Err("INVALID_ARGUMENT: project must not contain control characters".to_string());
    }
    Ok(folder.to_string())
}

/// The live app behind [`PickIo`].
pub struct AppIo;

impl PickIo for AppIo {
    fn agents(&self) -> Vec<AgentFact> {
        crate::tui::descriptors()
            .into_iter()
            .map(|d| AgentFact {
                id: d.id.to_string(),
                label: d.label.clone(),
                installed: d.installed,
                dispatchable: d.dispatchable,
                cli: d.kind == crate::tui::Kind::Cli.as_str(),
                vendor: crate::tui::profile_for(d.id).limits().map(|limits| limits.vendor().to_string()),
            })
            .collect()
    }

    fn runnable(&self, agent: &AgentFact, folder: &str) -> Result<(), String> {
        if !agent.installed {
            return Err("Not installed on this computer".to_string());
        }
        crate::companion::agents::agent_ok(&agent.id, folder).map_err(str::to_string)
    }

    fn account_ok(&self, agent_id: &str, account_id: &str) -> bool {
        crate::accounts::validate_for_run(agent_id, account_id).is_ok()
    }

    fn account_label(&self, account_id: &str) -> Option<String> {
        crate::accounts::label_of(account_id)
    }

    fn limit(&self, agent_id: &str, account: Option<&str>) -> Option<Option<u64>> {
        crate::handoff::agent_limit(agent_id, account)
    }

    fn rows(&self, folder: &str) -> Result<Vec<ScoreRow>, String> {
        crate::scores::agent_scores(Some(folder), Some(PICK_DAYS)).map(|report| report.rows)
    }

    fn usual(&self) -> Usual {
        let settings = crate::askbar::settings();
        Usual { preferred: settings.default_agent, last: settings.last_agent }
    }

    fn when(&self, ms: u64) -> String {
        crate::handoff::reset::when_text(ms.min(i64::MAX as u64) as i64, &crate::handoff::reset::SystemClock)
    }
}

/// The pick for a folder path (the only input of the command). Reads this computer's own history;
/// run it off the UI thread.
pub fn pick_for_folder(folder: &str) -> Result<Pick, String> {
    let folder = check_folder(folder)?;
    Ok(pick_for_in(&AppIo, &folder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scores::model::Sample;
    use crate::scores::stats::rate;
    use std::collections::{BTreeMap, BTreeSet};

    const FOLDER: &str = "C:\\Users\\you\\proj";

    fn agent(id: &str, vendor: Option<&str>) -> AgentFact {
        AgentFact {
            id: id.to_string(),
            label: id.to_string(),
            installed: true,
            dispatchable: true,
            cli: true,
            vendor: vendor.map(str::to_string),
        }
    }

    #[derive(Default)]
    struct Fake {
        agents: Vec<AgentFact>,
        rows: Vec<ScoreRow>,
        rows_fail: bool,
        /// (agent, login) -> limit
        limits: BTreeMap<(String, Option<String>), Option<u64>>,
        /// Saved logins that may run: (agent, login).
        logins: BTreeSet<(String, String)>,
        labels: BTreeMap<String, String>,
        unready: BTreeMap<String, String>,
        usual: Option<String>,
    }

    impl Fake {
        fn with(agents: Vec<AgentFact>) -> Self {
            Self { agents, ..Self::default() }
        }
        fn login(&mut self, agent: &str, id: &str, label: &str) {
            self.logins.insert((agent.to_string(), id.to_string()));
            self.labels.insert(id.to_string(), label.to_string());
        }
        fn limit_on(&mut self, agent: &str, login: Option<&str>, until: Option<u64>) {
            self.limits.insert((agent.to_string(), login.map(str::to_string)), until);
        }
    }

    impl PickIo for Fake {
        fn agents(&self) -> Vec<AgentFact> {
            self.agents.clone()
        }
        fn runnable(&self, agent: &AgentFact, _folder: &str) -> Result<(), String> {
            if !agent.installed {
                return Err("Not installed on this computer".to_string());
            }
            match self.unready.get(&agent.id) {
                Some(text) => Err(text.clone()),
                None => Ok(()),
            }
        }
        fn account_ok(&self, agent_id: &str, account_id: &str) -> bool {
            self.logins.contains(&(agent_id.to_string(), account_id.to_string()))
        }
        fn account_label(&self, account_id: &str) -> Option<String> {
            self.labels.get(account_id).cloned()
        }
        fn limit(&self, agent_id: &str, account: Option<&str>) -> Option<Option<u64>> {
            self.limits.get(&(agent_id.to_string(), account.map(str::to_string))).copied()
        }
        fn rows(&self, _folder: &str) -> Result<Vec<ScoreRow>, String> {
            if self.rows_fail {
                Err("INTERNAL: boom".to_string())
            } else {
                Ok(self.rows.clone())
            }
        }
        fn usual(&self) -> Usual {
            Usual { preferred: self.usual.clone(), last: None }
        }
        fn when(&self, ms: u64) -> String {
            format!("t{ms}")
        }
    }

    fn row(agent: &str, account: Option<&str>, turns: u32, green: u32) -> ScoreRow {
        let none = rate(0, 0);
        ScoreRow {
            agent_id: agent.to_string(),
            agent_label: agent.to_string(),
            account_id: account.map(str::to_string),
            account_label: String::new(),
            turns,
            enough: turns >= 5,
            ended_ok: none,
            checks_green: rate(green, turns),
            merged: none,
            undone: none,
            asked_you: none,
            per_merged: None,
            waiting_review: 0,
            turn_ms_median: None,
            lines_median: None,
            ahead: Vec::new(),
        }
    }

    fn chosen(pick: &Pick) -> (&str, Option<&str>) {
        let c = pick.choice.as_ref().expect("a choice");
        (c.agent_id.as_str(), c.account_id.as_deref())
    }

    #[test]
    fn only_dispatchable_cli_agents_become_candidates() {
        let mut service = agent("fake_svc", None);
        service.cli = false;
        let mut hidden = agent("fake_hidden", None);
        hidden.dispatchable = false;
        let io = Fake::with(vec![service, hidden, agent("fake_a", None)]);
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_a", None));
        assert_eq!(pick.considered.len(), 1);
    }

    #[test]
    fn wizard_and_store_style_ids_behave_like_any_other() {
        let io = Fake::with(vec![agent("wizard-made_1", Some("vendor_x")), agent("store.item-2", None)]);
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(pick.considered.len(), 2);
        assert!(pick.choice.is_some());
        let mut io = io;
        io.rows = vec![row("store.item-2", None, 10, 10), row("wizard-made_1", None, 10, 5)];
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("store.item-2", None));
    }

    #[test]
    fn one_candidate_per_agent_even_with_three_saved_logins() {
        let mut io = Fake::with(vec![agent("fake_a", None)]);
        io.login("fake_a", "w1", "Work");
        io.login("fake_a", "w2", "Home");
        io.rows = vec![row("fake_a", None, 3, 3), row("fake_a", Some("w1"), 6, 6), row("fake_a", Some("w2"), 4, 4)];
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(pick.considered.len(), 1);
        assert_eq!(chosen(&pick), ("fake_a", Some("w1")));
    }

    #[test]
    fn usual_login_comes_from_history_and_its_row_is_the_evidence() {
        let mut io = Fake::with(vec![agent("fake_a", None), agent("fake_b", None)]);
        io.login("fake_a", "w1", "Work");
        io.rows = vec![row("fake_a", None, 2, 0), row("fake_a", Some("w1"), 10, 10), row("fake_b", None, 10, 5)];
        let pick = pick_for_in(&io, FOLDER);
        let c = pick.choice.as_ref().unwrap();
        assert_eq!((c.agent_id.as_str(), c.account_id.as_deref()), ("fake_a", Some("w1")));
        assert!(c.label.contains("Work"), "label is {}", c.label);
        assert_eq!(pick.sample, Sample::Steady);
        assert!(pick.reason.contains("10 of 10"), "reason is {}", pick.reason);
    }

    #[test]
    fn a_login_without_history_here_is_never_suggested() {
        let mut io = Fake::with(vec![agent("fake_a", None)]);
        io.login("fake_a", "w1", "Work");
        io.rows = Vec::new();
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_a", None));
    }

    #[test]
    fn a_removed_login_falls_back_to_the_own_login() {
        let mut io = Fake::with(vec![agent("fake_a", None)]);
        io.rows = vec![row("fake_a", None, 2, 2), row("fake_a", Some("gone"), 9, 9)];
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_a", None));
    }

    #[test]
    fn limited_login_never_moves_to_another_login() {
        let mut io = Fake::with(vec![agent("fake_a", Some("vendor_x"))]);
        io.login("fake_a", "work", "Work");
        io.rows = vec![row("fake_a", Some("work"), 8, 8), row("fake_a", None, 3, 3)];
        io.limit_on("fake_a", Some("work"), Some(1_000));
        let pick = pick_for_in(&io, FOLDER);
        // The usual login is out of usage and the pick does not look for another login.
        assert!(pick.choice.is_none(), "choice is {:?}", pick.choice);
        assert_eq!(pick.considered.len(), 1);
        assert!(pick.considered.iter().all(|c| !c.label.is_empty()));
        // The own login was never offered as a second candidate.
        assert_eq!(pick.considered[0].label, "fake_a · Work");
    }

    #[test]
    fn a_limit_on_a_login_not_used_here_does_not_matter() {
        let mut io = Fake::with(vec![agent("fake_a", None)]);
        io.login("fake_a", "other", "Other");
        io.rows = vec![row("fake_a", None, 6, 6)];
        io.limit_on("fake_a", Some("other"), Some(5));
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_a", None));
    }

    #[test]
    fn same_vendor_agent_is_skipped_when_one_is_out() {
        let mut io = Fake::with(vec![agent("fake_a", Some("vendor_x")), agent("fake_b", Some("vendor_x")), agent("fake_c", None)]);
        io.limit_on("fake_a", None, Some(1_000));
        io.rows = vec![row("fake_a", None, 10, 10), row("fake_b", None, 10, 9), row("fake_c", None, 10, 5)];
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_c", None));
        let skipped: Vec<&str> =
            pick.considered.iter().filter(|c| c.state == super::super::model::ConsideredState::Skipped).map(|c| c.label.as_str()).collect();
        assert_eq!(skipped, vec!["fake_a", "fake_b"]);
    }

    #[test]
    fn varies_and_empty_vendors_never_share_a_plan() {
        let mut io = Fake::with(vec![agent("fake_a", Some("varies")), agent("fake_b", Some("")), agent("fake_c", Some("varies"))]);
        io.limit_on("fake_a", None, None);
        let pick = pick_for_in(&io, FOLDER);
        let ready = pick.considered.iter().filter(|c| c.state == super::super::model::ConsideredState::Ready).count();
        assert_eq!(ready, 2);
    }

    #[test]
    fn unreadable_history_adds_the_note_and_picks_the_usual_agent() {
        let mut io = Fake::with(vec![agent("fake_a", None), agent("fake_b", None)]);
        io.rows_fail = true;
        io.usual = Some("fake_b".to_string());
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_b", None));
        assert_eq!(pick.notes.len(), 4);
        assert_eq!(pick.notes[3], UNREADABLE_NOTE);
        assert!(pick.reason.starts_with("Too early"), "reason is {}", pick.reason);
    }

    #[test]
    fn readable_history_has_three_notes() {
        let io = Fake::with(vec![agent("fake_a", None)]);
        assert_eq!(pick_for_in(&io, FOLDER).notes.len(), 3);
    }

    #[test]
    fn an_unapproved_agent_is_skipped_with_the_approval_text() {
        let mut io = Fake::with(vec![agent("fake_a", None), agent("fake_b", None)]);
        io.unready.insert("fake_a".to_string(), "Approve this agent in Settings > Agents first.".to_string());
        io.rows = vec![row("fake_a", None, 10, 10), row("fake_b", None, 10, 4)];
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_b", None));
        let line = pick.considered.iter().find(|c| c.label == "fake_a").map(|c| c.line.clone()).unwrap();
        assert_eq!(line, "Approve this agent in Settings > Agents first.");
    }

    #[test]
    fn not_installed_is_skipped() {
        let mut gone = agent("fake_a", None);
        gone.installed = false;
        let io = Fake::with(vec![gone, agent("fake_b", None)]);
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(chosen(&pick), ("fake_b", None));
        assert!(pick.considered.iter().any(|c| c.line == "Not installed on this computer"));
    }

    #[test]
    fn the_usual_agent_comes_from_the_settings() {
        let mut io = Fake::with(vec![agent("fake_a", None), agent("fake_b", None)]);
        io.usual = Some("FAKE_B".to_string());
        assert_eq!(chosen(&pick_for_in(&io, FOLDER)), ("fake_b", None));
    }

    #[test]
    fn a_clear_gap_names_both_counts() {
        let mut io = Fake::with(vec![agent("fake_a", None), agent("fake_b", None)]);
        io.rows = vec![row("fake_a", None, 10, 10), row("fake_b", None, 10, 5)];
        let pick = pick_for_in(&io, FOLDER);
        assert_eq!(pick.reason, "fake_a passed its checks 10 of 10 times here; fake_b 5 of 10.");
    }

    #[test]
    fn no_agent_at_all() {
        let io = Fake::with(Vec::new());
        let pick = pick_for_in(&io, FOLDER);
        assert!(pick.choice.is_none());
        assert_eq!(pick.reason, "No agent is installed yet.");
    }

    #[test]
    fn the_folder_check_accepts_a_plain_path_and_trims_it() {
        assert_eq!(check_folder("  C:\\Users\\you\\proj \n").unwrap(), "C:\\Users\\you\\proj");
        assert!(check_folder(&"a".repeat(1024)).is_ok());
    }

    #[test]
    fn the_folder_check_rejects_bad_input() {
        for bad in ["", "   ", &"a".repeat(1025), "C:\\proj\u{0}x", "a\nb", "a\u{7f}b"] {
            let err = check_folder(bad).unwrap_err();
            assert!(err.starts_with("INVALID_ARGUMENT: project"), "{err}");
        }
    }

    #[test]
    fn pick_for_folder_rejects_bad_input_before_reading_anything() {
        assert!(pick_for_folder("").unwrap_err().starts_with("INVALID_ARGUMENT: project"));
    }
}
