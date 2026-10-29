//! Where a pane runs (SPEC-n33 sections 3.1 and 3.6 C). `decide` picks Windows or Linux for a spawn
//! from the setting, the one-shot request and the pane's facts. `resolve` turns a chosen distro into
//! a `LinuxPlace` (distro listed and WSL 2, folder mapped, program found). `check_run` is the early
//! check `run_agent` makes with the live settings and the probe. Nothing here starts a process: the
//! probe comes in through `PlaceProbe`, and the settings through `settings::Store`.

use crate::operations::OperationError;
use crate::tui::{self, Kind};
use crate::wsl::model::{
    Distro, DistroFacts, DistroName, LinuxPlace, PanePlace, WslStatus, FACTS_FRESH_MS, LIST_FRESH_MS,
    WSL_ACCOUNT_NOT_YET, WSL_AGENT_NOT_FOUND, WSL_AGENT_NOT_SUPPORTED, WSL_COPY_NOT_YET,
    WSL_DISTRO_MISSING, WSL_DISTRO_NOT_V2, WSL_GUARD_NOT_SUPPORTED, WSL_HEADLESS_NOT_YET,
    WSL_NOT_INSTALLED, WSL_NO_DISTRO, WSL_PATH_NOT_MAPPABLE, WSL_START_FAILED,
};
use crate::wsl::path::{self, Mounts, PathError};
use crate::wsl::settings::Store;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Panes whose id starts with this are companions. They always run on Windows (peer guard).
const COMPANION_PREFIX: &str = "companion-";
/// How long a one-shot distro request waits for its spawn.
pub const ONCE_TTL: Duration = Duration::from_secs(60);

/// What the spawn (or the run check) knows about the pane it starts.
pub struct Ask<'a> {
    pub agent_type: &'a str,
    pub pane_id: &'a str,
    /// A saved account (its own agent home). Not supported in Linux yet.
    pub account: Option<&'a str>,
    /// The env guard of a guarded launch. Not supported in Linux yet.
    pub guard_env: Option<&'a str>,
    /// A hidden pane (standby, preload). Always Windows.
    pub hidden: bool,
    /// A separate copy (`isolation: worktree`). Not supported in Linux yet.
    pub copy: bool,
    pub headless: Headless,
    pub kind: Kind,
    /// The bare program the profile runs in Linux (`tui::linux_program`); None when it has no Linux launch.
    pub linux_program: Option<String>,
}

/// How the run is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Headless {
    /// A pane.
    No,
    /// The caller asked for a one-task run.
    Explicit,
    /// The agent's run mode is one-task and the caller did not say.
    FromRunMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Windows,
    /// `visible_instead_of_one_task`: a one-task run becomes a visible pane in Linux.
    Linux { distro: String, visible_instead_of_one_task: bool },
}

/// Windows or Linux for one spawn. No setting and no one-shot request means Windows (today's path).
/// Companion and hidden panes always stay on Windows. A one-shot request beats the setting. Then the
/// refusals: an agent without a Linux launch, a saved account, an env guard, a separate copy, and an
/// explicit one-task run.
pub fn decide(ask: &Ask, setting: Option<&str>, once: Option<&str>) -> Result<Decision, OperationError> {
    if ask.hidden || ask.pane_id.starts_with(COMPANION_PREFIX) {
        return Ok(Decision::Windows);
    }
    let Some(distro_text) = once.or(setting) else {
        return Ok(Decision::Windows);
    };
    let distro = DistroName::parse(distro_text).ok_or_else(|| distro_missing(distro_text))?;
    if ask.kind != Kind::Cli || ask.linux_program.is_none() {
        return Err(refuse(
            WSL_AGENT_NOT_SUPPORTED,
            format!("{} can't run in Linux from Puppet Master yet.", label_for(ask.agent_type)),
        ));
    }
    if ask.account.is_some() {
        return Err(refuse(
            WSL_ACCOUNT_NOT_YET,
            format!(
                "Saved logins don't work in Linux yet. Start it without one, or run {} on Windows.",
                label_for(ask.agent_type)
            ),
        ));
    }
    if ask.guard_env.is_some() {
        return Err(refuse(
            WSL_GUARD_NOT_SUPPORTED,
            format!("This feature runs agents on Windows only. Set {} to run on Windows to use it.", label_for(ask.agent_type)),
        ));
    }
    if ask.copy {
        return Err(refuse(
            WSL_COPY_NOT_YET,
            format!(
                "Separate copies don't work in Linux yet. Turn off Separate copy, or run {} on Windows.",
                label_for(ask.agent_type)
            ),
        ));
    }
    let visible_instead_of_one_task = match ask.headless {
        Headless::No => false,
        Headless::FromRunMode => true,
        Headless::Explicit => {
            return Err(refuse(
                WSL_HEADLESS_NOT_YET,
                format!("One-task runs don't work in Linux yet. Use a pane, or run {} on Windows.", label_for(ask.agent_type)),
            ))
        }
    };
    Ok(Decision::Linux { distro: distro.as_str().to_string(), visible_instead_of_one_task })
}

/// The place of one Linux pane, from the distro the person chose and the probe facts for it.
/// `windows_cwd` is the pane's folder on Windows; `program` is the bare program name.
pub fn resolve(
    distro: &str,
    status: &WslStatus,
    facts: &DistroFacts,
    windows_cwd: &str,
    program: &str,
) -> Result<LinuxPlace, OperationError> {
    let found = listed_distro(distro, status)?;
    let mounts = Mounts {
        distro: found.name.clone(),
        mount_root: facts.mount_root.clone(),
        unc_root: format!(r"\\wsl.localhost\{}", found.name),
    };
    let linux_cwd = path::to_linux(windows_cwd, &mounts).map_err(path_refusal)?;
    let program_abs = match facts.programs.get(program) {
        Some(Some(abs)) if abs.starts_with('/') => abs.clone(),
        _ => {
            return Err(refuse(
                WSL_AGENT_NOT_FOUND,
                format!(
                    "{} is not installed in {}. Install it inside Linux, then try again.",
                    label_for_program(program),
                    found.name
                ),
            ))
        }
    };
    Ok(LinuxPlace {
        distro: found.name.clone(),
        linux_cwd,
        program_abs,
        path: facts.path.clone(),
        mounts,
        home: facts.home.clone(),
        home_windows: facts.home_windows.clone(),
    })
}

/// The `place` field of `PaneInfo` for a Linux pane. Display and chat facts only.
pub fn pane_place(place: &LinuxPlace) -> PanePlace {
    PanePlace {
        kind: "linux".to_string(),
        distro: place.distro.clone(),
        cwd: place.linux_cwd.clone(),
        mount_root: place.mounts.mount_root.clone(),
        unc_root: place.mounts.unc_root.clone(),
        home_windows: place.home_windows.clone(),
        warnings: Vec::new(),
    }
}

/// One-shot distro requests, keyed by pane id (set by `spawn_pane_in_linux` before the spawn). Never
/// in the JSON of a spawn request, so nothing from outside can set it.
static ONCE: Mutex<BTreeMap<String, (Instant, String)>> = Mutex::new(BTreeMap::new());

fn once_map() -> MutexGuard<'static, BTreeMap<String, (Instant, String)>> {
    ONCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn request_once(pane_id: &str, distro: &DistroName) {
    request_once_at(pane_id, distro, Instant::now());
}

fn request_once_at(pane_id: &str, distro: &DistroName, now: Instant) {
    let mut map = once_map();
    map.retain(|_, (at, _)| now.saturating_duration_since(*at) < ONCE_TTL);
    map.insert(pane_id.to_string(), (now, distro.as_str().to_string()));
}

/// The one-shot distro for this pane, if one was requested less than `ONCE_TTL` ago. Taking it
/// removes it, so it applies to one spawn only.
pub fn take_once(pane_id: &str) -> Option<String> {
    take_once_at(pane_id, Instant::now())
}

fn take_once_at(pane_id: &str, now: Instant) -> Option<String> {
    let (at, distro) = once_map().remove(pane_id)?;
    (now.saturating_duration_since(at) < ONCE_TTL).then_some(distro)
}

/// What `check_run` reads from the probe: the distro list (cached, never boots a distro) and one
/// distro's facts (probed on a miss, which boots it). `probe::cache()` implements this.
pub trait PlaceProbe {
    fn status(&self, max_age_ms: u64) -> WslStatus;
    fn facts(&self, distro: &str, max_age_ms: u64) -> Result<DistroFacts, OperationError>;
}

/// The live probe: the process-wide cache of `wsl/probe.rs`.
fn live_probe() -> &'static dyn PlaceProbe {
    crate::wsl::probe::cache()
}

#[cfg(not(test))]
fn settings_store() -> Store {
    Store::live()
}

/// Tests never read the person's settings file: a path that does not exist means every agent on Windows.
#[cfg(test)]
fn settings_store() -> Store {
    Store::at(&std::env::temp_dir().join("pm-wsl-place-no-settings"))
}

/// The place of a pane that is about to spawn: `None` = Windows (today's path, nothing read from the
/// probe), `Some` = Linux. Reads the live settings and takes the one-shot request for this pane.
pub fn plan_spawn(ask: &Ask<'_>, windows_cwd: &str) -> Result<Option<LinuxPlace>, OperationError> {
    plan_with(&settings_store(), live_probe(), ask, take_once(ask.pane_id).as_deref(), windows_cwd)
}

/// `plan_spawn` with the settings, the probe and the one-shot request passed in (tests use fakes).
pub fn plan_with(
    store: &Store,
    probe: &dyn PlaceProbe,
    ask: &Ask<'_>,
    once: Option<&str>,
    windows_cwd: &str,
) -> Result<Option<LinuxPlace>, OperationError> {
    let Decision::Linux { distro, .. } = decide(ask, store.get(ask.agent_type).as_deref(), once)? else {
        return Ok(None);
    };
    let status = probe.status(LIST_FRESH_MS);
    listed_distro(&distro, &status)?;
    let facts = probe.facts(&distro, FACTS_FRESH_MS)?;
    resolve(&distro, &status, &facts, windows_cwd, ask.linux_program.as_deref().unwrap_or("")).map(Some)
}

/// The early check `run_agent` makes (SPEC 3.6 C): the same rules as `decide` and `resolve`, with
/// the live settings and the probe. Refusals come back before any operation or pane exists.
pub fn check_run(
    agent_type: &str,
    project: &str,
    copy: bool,
    headless: Headless,
    account: Option<&str>,
) -> Result<Decision, OperationError> {
    let ask = Ask {
        agent_type,
        pane_id: "",
        account,
        guard_env: None,
        hidden: false,
        copy,
        headless,
        kind: tui::profile_for(agent_type).features().kind,
        linux_program: tui::linux_program(agent_type),
    };
    check_run_with(&settings_store(), live_probe(), &ask, project)
}

/// `check_run` with the settings and the probe passed in (tests use fakes).
pub fn check_run_with(
    store: &Store,
    probe: &dyn PlaceProbe,
    ask: &Ask<'_>,
    project: &str,
) -> Result<Decision, OperationError> {
    let decision = decide(ask, store.get(ask.agent_type).as_deref(), None)?;
    if let Decision::Linux { distro, .. } = &decision {
        // Refuse a missing or old distro before the probe boots it.
        let status = probe.status(LIST_FRESH_MS);
        listed_distro(distro, &status)?;
        let facts = probe.facts(distro, FACTS_FRESH_MS)?;
        resolve(distro, &status, &facts, project, ask.linux_program.as_deref().unwrap_or(""))?;
    }
    Ok(decision)
}

/// Whether this agent's place setting names a distro, so its runs are Linux runs. Reads the setting
/// only: nothing is probed or started. The fallback and standby rules use it (SPEC WSL-15).
pub fn agent_runs_in_linux(agent_type: &str) -> bool {
    agent_runs_in_linux_with(&settings_store(), agent_type)
}

/// `agent_runs_in_linux` with the settings passed in (tests use a fake store).
pub fn agent_runs_in_linux_with(store: &Store, agent_type: &str) -> bool {
    store.get(agent_type).is_some()
}

/// The listed distro that `distro` names (case-insensitive), when the status allows a Linux pane.
fn listed_distro<'a>(distro: &str, status: &'a WslStatus) -> Result<&'a Distro, OperationError> {
    let distros = match status {
        WslStatus::NotWindows => {
            return Err(refuse(WSL_NOT_INSTALLED, "Linux is not available on this computer.".to_string()))
        }
        WslStatus::NotInstalled { command } => {
            return Err(refuse(
                WSL_NOT_INSTALLED,
                format!("WSL is not installed. Run \"{command}\" in PowerShell as administrator, then try again."),
            ))
        }
        WslStatus::NoDistro { command } => {
            return Err(refuse(WSL_NO_DISTRO, format!("WSL has no Linux yet. Run \"{command}\", then try again.")))
        }
        WslStatus::Broken { line } => {
            return Err(OperationError::new(WSL_START_FAILED, format!("Linux did not start: {line}"), true))
        }
        WslStatus::Ready { distros } => distros,
    };
    let found = distros
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(distro))
        .ok_or_else(|| distro_missing(distro))?;
    if found.version != 2 {
        return Err(refuse(WSL_DISTRO_NOT_V2, format!("{} uses WSL 1. Agents need WSL 2.", found.name)));
    }
    Ok(found)
}

fn path_refusal(err: PathError) -> OperationError {
    let text = match err {
        PathError::InsideDistro | PathError::OtherDistro(_) => {
            "Folders inside Linux are not supported yet. Use a folder on a Windows drive."
        }
        _ => "This folder can't be opened from Linux. Use a folder on a Windows drive, like C: or D:.",
    };
    refuse(WSL_PATH_NOT_MAPPABLE, text.to_string())
}

fn distro_missing(distro: &str) -> OperationError {
    refuse(
        WSL_DISTRO_MISSING,
        format!("Linux \"{distro}\" was not found. Pick another one in Settings > Agents."),
    )
}

fn refuse(code: &str, message: String) -> OperationError {
    OperationError::new(code, message, false)
}

/// The display name of a registered agent, or its id when it is not registered.
fn label_for(agent_type: &str) -> String {
    tui::descriptor_for(agent_type).map(|descriptor| descriptor.label).unwrap_or_else(|| agent_type.to_string())
}

/// The display name of the registered agent whose Linux program is `program`, or the program name.
fn label_for_program(program: &str) -> String {
    tui::descriptors()
        .into_iter()
        .find(|descriptor| tui::linux_program(descriptor.id).as_deref() == Some(program))
        .map(|descriptor| descriptor.label)
        .unwrap_or_else(|| program.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wsl::model::{ADD_DISTRO_COMMAND, INSTALL_COMMAND, WSL_PROBE_TIMEOUT};
    use std::cell::Cell;

    /// A Windows-side pane that runs one agent in Linux (the agent id is a test id, not a real agent).
    fn cli_ask(agent: &str) -> Ask<'_> {
        Ask {
            agent_type: agent,
            pane_id: "pane-1",
            account: None,
            guard_env: None,
            hidden: false,
            copy: false,
            headless: Headless::No,
            kind: Kind::Cli,
            linux_program: Some("fake-cli".to_string()),
        }
    }

    fn linux(distro: &str, visible: bool) -> Decision {
        Decision::Linux { distro: distro.to_string(), visible_instead_of_one_task: visible }
    }

    fn code_of(result: Result<Decision, OperationError>) -> String {
        result.unwrap_err().code
    }

    fn sample_facts(distro: &str) -> DistroFacts {
        DistroFacts {
            distro: distro.to_string(),
            checked_at_ms: 1_000,
            uid: 1000,
            home: "/home/you".to_string(),
            home_windows: r"\\wsl.localhost\Ubuntu\home\you".to_string(),
            mount_root: "/mnt/".to_string(),
            path: "/usr/bin:/bin".to_string(),
            git: Some("2.43.0".to_string()),
            interop: true,
            networking: Some("nat".to_string()),
            programs: BTreeMap::from([
                ("fake-cli".to_string(), Some("/usr/bin/fake-cli".to_string())),
                ("missing-cli".to_string(), None),
            ]),
        }
    }

    fn ready(name: &str, version: u8) -> WslStatus {
        WslStatus::Ready {
            distros: vec![Distro { name: name.to_string(), default: true, version, state: "Running".to_string() }],
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-wsl-place-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---- decide ----

    #[test]
    fn no_setting_and_no_one_shot_means_windows() {
        assert_eq!(decide(&cli_ask("fake-cli"), None, None).unwrap(), Decision::Windows);
        // Windows keeps the refusal-free path too: nothing in the ask is checked.
        let mut ask = cli_ask("fake-cli");
        ask.copy = true;
        ask.headless = Headless::Explicit;
        ask.account = Some("work");
        ask.guard_env = Some("NO_PUSH");
        assert_eq!(decide(&ask, None, None).unwrap(), Decision::Windows);
    }

    #[test]
    fn a_setting_puts_the_pane_in_linux() {
        assert_eq!(decide(&cli_ask("fake-cli"), Some("Ubuntu"), None).unwrap(), linux("Ubuntu", false));
    }

    #[test]
    fn companion_and_hidden_panes_stay_on_windows() {
        let mut companion = cli_ask("fake-cli");
        companion.pane_id = "companion-mochi";
        companion.account = Some("work");
        companion.copy = true;
        assert_eq!(decide(&companion, Some("Ubuntu"), Some("Debian")).unwrap(), Decision::Windows);
        let mut hidden = cli_ask("fake-cli");
        hidden.hidden = true;
        assert_eq!(decide(&hidden, Some("Ubuntu"), None).unwrap(), Decision::Windows);
    }

    #[test]
    fn a_one_shot_request_beats_the_setting() {
        assert_eq!(decide(&cli_ask("fake-cli"), Some("Ubuntu"), Some("Debian")).unwrap(), linux("Debian", false));
        assert_eq!(decide(&cli_ask("fake-cli"), None, Some("Debian")).unwrap(), linux("Debian", false));
    }

    #[test]
    fn one_task_runs_from_the_run_mode_become_visible() {
        let mut ask = cli_ask("fake-cli");
        ask.headless = Headless::FromRunMode;
        assert_eq!(decide(&ask, Some("Ubuntu"), None).unwrap(), linux("Ubuntu", true));
        ask.headless = Headless::No;
        assert_eq!(decide(&ask, Some("Ubuntu"), None).unwrap(), linux("Ubuntu", false));
    }

    #[test]
    fn an_explicit_one_task_run_is_refused_in_linux() {
        let mut ask = cli_ask("fake-cli");
        ask.headless = Headless::Explicit;
        assert_eq!(code_of(decide(&ask, Some("Ubuntu"), None)), WSL_HEADLESS_NOT_YET);
        // The same run on Windows is fine.
        assert_eq!(decide(&ask, None, None).unwrap(), Decision::Windows);
    }

    #[test]
    fn linux_refusals_have_their_codes_and_name_the_agent() {
        let mut copy = cli_ask("fake-cli");
        copy.copy = true;
        assert_eq!(code_of(decide(&copy, Some("Ubuntu"), None)), WSL_COPY_NOT_YET);

        let mut account = cli_ask("fake-cli");
        account.account = Some("work");
        assert_eq!(code_of(decide(&account, Some("Ubuntu"), None)), WSL_ACCOUNT_NOT_YET);

        let mut guard = cli_ask("fake-cli");
        guard.guard_env = Some("NO_PUSH");
        assert_eq!(code_of(decide(&guard, Some("Ubuntu"), None)), WSL_GUARD_NOT_SUPPORTED);

        let err = decide(&copy, Some("Ubuntu"), None).unwrap_err();
        assert!(err.message.starts_with("Separate copies don't work in Linux yet."), "{}", err.message);
        assert!(err.message.contains("fake-cli"), "{}", err.message);
        assert!(!err.recoverable);
    }

    #[test]
    fn agents_without_a_linux_launch_are_not_supported() {
        for kind in [Kind::NativeProtocol, Kind::Shell, Kind::Unknown] {
            let mut ask = cli_ask("fake-cli");
            ask.kind = kind;
            assert_eq!(code_of(decide(&ask, Some("Ubuntu"), None)), WSL_AGENT_NOT_SUPPORTED, "{kind:?}");
        }
        let mut no_program = cli_ask("fake-cli");
        no_program.linux_program = None;
        assert_eq!(code_of(decide(&no_program, Some("Ubuntu"), None)), WSL_AGENT_NOT_SUPPORTED);
        let err = decide(&no_program, Some("Ubuntu"), None).unwrap_err();
        assert_eq!(err.message, "fake-cli can't run in Linux from Puppet Master yet.");
    }

    #[test]
    fn a_bad_distro_name_is_reported_as_missing() {
        assert_eq!(code_of(decide(&cli_ask("fake-cli"), Some("a b"), None)), WSL_DISTRO_MISSING);
        assert_eq!(code_of(decide(&cli_ask("fake-cli"), None, Some("../x"))), WSL_DISTRO_MISSING);
    }

    #[test]
    fn a_one_shot_request_expires_after_a_minute_and_is_taken_once() {
        let distro = DistroName::parse("Ubuntu").unwrap();
        let base = Instant::now();
        request_once_at("place-test-expiry", &distro, base);
        assert_eq!(take_once_at("place-test-expiry", base + Duration::from_secs(61)), None);

        request_once_at("place-test-take", &distro, base);
        assert_eq!(take_once_at("place-test-take", base + Duration::from_secs(59)).as_deref(), Some("Ubuntu"));
        assert_eq!(take_once_at("place-test-take", base + Duration::from_secs(59)), None, "one spawn only");

        // The public pair uses the real clock: a fresh request is there at once.
        request_once("place-test-public", &distro);
        assert_eq!(take_once("place-test-public").as_deref(), Some("Ubuntu"));
        assert_eq!(take_once("place-test-never-set"), None);
    }

    // ---- resolve ----

    #[test]
    fn resolve_maps_the_folder_and_finds_the_program() {
        let place = resolve("Ubuntu", &ready("Ubuntu", 2), &sample_facts("Ubuntu"), r"D:\work\shop", "fake-cli").unwrap();
        assert_eq!(place.distro, "Ubuntu");
        assert_eq!(place.linux_cwd, "/mnt/d/work/shop");
        assert_eq!(place.program_abs, "/usr/bin/fake-cli");
        assert_eq!(place.path, "/usr/bin:/bin");
        assert_eq!(place.home, "/home/you");
        assert_eq!(place.home_windows, r"\\wsl.localhost\Ubuntu\home\you");
        assert_eq!(
            place.mounts,
            Mounts { distro: "Ubuntu".into(), mount_root: "/mnt/".into(), unc_root: r"\\wsl.localhost\Ubuntu".into() }
        );
    }

    #[test]
    fn the_distro_match_is_case_insensitive_and_uses_the_listed_name() {
        let place = resolve("ubuntu", &ready("Ubuntu", 2), &sample_facts("Ubuntu"), r"C:\x", "fake-cli").unwrap();
        assert_eq!(place.distro, "Ubuntu");
        assert_eq!(place.mounts.unc_root, r"\\wsl.localhost\Ubuntu");
    }

    #[test]
    fn a_mount_root_of_slash_maps_the_drive_without_mnt() {
        let mut facts = sample_facts("Ubuntu");
        facts.mount_root = "/".to_string();
        let place = resolve("Ubuntu", &ready("Ubuntu", 2), &facts, r"C:\work\shop", "fake-cli").unwrap();
        assert_eq!(place.linux_cwd, "/c/work/shop");
    }

    #[test]
    fn a_missing_distro_or_a_wsl1_distro_is_refused() {
        let err = resolve("Debian", &ready("Ubuntu", 2), &sample_facts("Debian"), r"C:\x", "fake-cli").unwrap_err();
        assert_eq!(err.code, WSL_DISTRO_MISSING);
        assert_eq!(err.message, r#"Linux "Debian" was not found. Pick another one in Settings > Agents."#);

        let err = resolve("Ubuntu", &ready("Ubuntu", 1), &sample_facts("Ubuntu"), r"C:\x", "fake-cli").unwrap_err();
        assert_eq!(err.code, WSL_DISTRO_NOT_V2);
        assert_eq!(err.message, "Ubuntu uses WSL 1. Agents need WSL 2.");
    }

    #[test]
    fn a_status_that_is_not_ready_is_refused_with_its_code() {
        let facts = sample_facts("Ubuntu");
        let not_installed = WslStatus::NotInstalled { command: INSTALL_COMMAND.to_string() };
        let err = resolve("Ubuntu", &not_installed, &facts, r"C:\x", "fake-cli").unwrap_err();
        assert_eq!(err.code, WSL_NOT_INSTALLED);
        assert!(err.message.contains(INSTALL_COMMAND), "{}", err.message);

        let no_distro = WslStatus::NoDistro { command: ADD_DISTRO_COMMAND.to_string() };
        let err = resolve("Ubuntu", &no_distro, &facts, r"C:\x", "fake-cli").unwrap_err();
        assert_eq!(err.code, WSL_NO_DISTRO);
        assert_eq!(err.message, r#"WSL has no Linux yet. Run "wsl --install -d Ubuntu", then try again."#);

        let broken = WslStatus::Broken { line: "boom".to_string() };
        let err = resolve("Ubuntu", &broken, &facts, r"C:\x", "fake-cli").unwrap_err();
        assert_eq!(err.code, WSL_START_FAILED);
        assert_eq!(err.message, "Linux did not start: boom");
        assert!(err.recoverable, "a failed start can be tried again");

        let err = resolve("Ubuntu", &WslStatus::NotWindows, &facts, r"C:\x", "fake-cli").unwrap_err();
        assert_eq!(err.code, WSL_NOT_INSTALLED);
    }

    #[test]
    fn folders_that_cannot_map_are_refused_with_the_right_sentence() {
        let facts = sample_facts("Ubuntu");
        let status = ready("Ubuntu", 2);
        for inside in [r"\\wsl.localhost\Ubuntu\home\you\shop", r"\\wsl.localhost\Debian\x"] {
            let err = resolve("Ubuntu", &status, &facts, inside, "fake-cli").unwrap_err();
            assert_eq!(err.code, WSL_PATH_NOT_MAPPABLE, "{inside}");
            assert_eq!(
                err.message,
                "Folders inside Linux are not supported yet. Use a folder on a Windows drive.",
                "{inside}"
            );
        }
        for outside in [r"\\server\share\x", "shop", r"C:\a\..\b", ""] {
            let err = resolve("Ubuntu", &status, &facts, outside, "fake-cli").unwrap_err();
            assert_eq!(err.code, WSL_PATH_NOT_MAPPABLE, "{outside:?}");
            assert_eq!(
                err.message,
                "This folder can't be opened from Linux. Use a folder on a Windows drive, like C: or D:.",
                "{outside:?}"
            );
        }
    }

    #[test]
    fn a_missing_program_is_refused_and_named_by_the_registry_label() {
        let facts = sample_facts("Ubuntu");
        let status = ready("Ubuntu", 2);
        // `missing-cli` is listed with no path; `absent-cli` was never asked for.
        for program in ["missing-cli", "absent-cli"] {
            let err = resolve("Ubuntu", &status, &facts, r"C:\x", program).unwrap_err();
            assert_eq!(err.code, WSL_AGENT_NOT_FOUND, "{program}");
            assert_eq!(
                err.message,
                format!("{program} is not installed in Ubuntu. Install it inside Linux, then try again.")
            );
        }
        // A real registered agent with a Linux program is named by its label.
        let (label, program) = tui::descriptors()
            .into_iter()
            .find_map(|descriptor| tui::linux_program(descriptor.id).map(|program| (descriptor.label, program)))
            .expect("one registered agent has a Linux program");
        let err = resolve("Ubuntu", &status, &facts, r"C:\x", &program).unwrap_err();
        assert!(err.message.starts_with(&format!("{label} is not installed in Ubuntu.")), "{}", err.message);
    }

    #[test]
    fn a_resolved_place_becomes_pane_facts_with_no_warnings() {
        let place = resolve("Ubuntu", &ready("Ubuntu", 2), &sample_facts("Ubuntu"), r"D:\work\shop", "fake-cli").unwrap();
        let pane = pane_place(&place);
        assert_eq!(pane.kind, "linux");
        assert_eq!(pane.distro, "Ubuntu");
        assert_eq!(pane.cwd, "/mnt/d/work/shop");
        assert_eq!(pane.mount_root, "/mnt/");
        assert_eq!(pane.unc_root, r"\\wsl.localhost\Ubuntu");
        assert_eq!(pane.home_windows, r"\\wsl.localhost\Ubuntu\home\you");
        assert!(pane.warnings.is_empty());
    }

    // ---- check_run (the settings and the probe are fakes) ----

    struct FakeProbe {
        status: WslStatus,
        timeout: bool,
        facts_calls: Cell<u32>,
    }

    impl FakeProbe {
        fn new(status: WslStatus) -> Self {
            Self { status, timeout: false, facts_calls: Cell::new(0) }
        }
    }

    impl PlaceProbe for FakeProbe {
        fn status(&self, _max_age_ms: u64) -> WslStatus {
            self.status.clone()
        }

        fn facts(&self, distro: &str, _max_age_ms: u64) -> Result<DistroFacts, OperationError> {
            self.facts_calls.set(self.facts_calls.get() + 1);
            if self.timeout {
                return Err(OperationError::new(WSL_PROBE_TIMEOUT, "slow", true));
            }
            Ok(sample_facts(distro))
        }
    }

    /// A settings file that sends `fake-cli` to Ubuntu.
    fn store_with_ubuntu(name: &str) -> Store {
        let store = Store::at(&scratch(name));
        store.set("fake-cli", Some(&DistroName::parse("Ubuntu").unwrap())).unwrap();
        store
    }

    #[test]
    fn check_run_without_a_setting_never_probes() {
        let store = Store::at(&scratch("check-windows"));
        let probe = FakeProbe::new(ready("Ubuntu", 2));
        let decision = check_run_with(&store, &probe, &cli_ask("fake-cli"), r"D:\work\shop").unwrap();
        assert_eq!(decision, Decision::Windows);
        assert_eq!(probe.facts_calls.get(), 0);
    }

    #[test]
    fn check_run_with_a_setting_checks_the_distro_and_the_folder() {
        let store = store_with_ubuntu("check-linux");
        let probe = FakeProbe::new(ready("Ubuntu", 2));
        let decision = check_run_with(&store, &probe, &cli_ask("fake-cli"), r"D:\work\shop").unwrap();
        assert_eq!(decision, linux("Ubuntu", false));
        assert_eq!(probe.facts_calls.get(), 1);
    }

    #[test]
    fn check_run_refuses_before_the_probe_when_linux_is_not_ready() {
        let store = store_with_ubuntu("check-not-installed");
        let probe = FakeProbe::new(WslStatus::NotInstalled { command: INSTALL_COMMAND.to_string() });
        let err = check_run_with(&store, &probe, &cli_ask("fake-cli"), r"D:\x").unwrap_err();
        assert_eq!(err.code, WSL_NOT_INSTALLED);
        assert_eq!(probe.facts_calls.get(), 0, "no boot for a missing WSL");
    }

    #[test]
    fn check_run_refuses_a_policy_before_the_probe() {
        let store = store_with_ubuntu("check-account");
        let probe = FakeProbe::new(ready("Ubuntu", 2));
        let mut ask = cli_ask("fake-cli");
        ask.account = Some("work");
        assert_eq!(check_run_with(&store, &probe, &ask, r"D:\x").unwrap_err().code, WSL_ACCOUNT_NOT_YET);
        assert_eq!(probe.facts_calls.get(), 0);
    }

    #[test]
    fn check_run_passes_probe_timeouts_and_folder_refusals_through() {
        let store = store_with_ubuntu("check-timeout");
        let mut probe = FakeProbe::new(ready("Ubuntu", 2));
        probe.timeout = true;
        assert_eq!(check_run_with(&store, &probe, &cli_ask("fake-cli"), r"D:\x").unwrap_err().code, WSL_PROBE_TIMEOUT);

        let probe = FakeProbe::new(ready("Ubuntu", 2));
        let err = check_run_with(&store, &probe, &cli_ask("fake-cli"), r"\\wsl.localhost\Ubuntu\home\you").unwrap_err();
        assert_eq!(err.code, WSL_PATH_NOT_MAPPABLE);
        assert_eq!(probe.facts_calls.get(), 1, "the folder is judged with the facts");
    }

    #[test]
    fn check_run_keeps_the_one_task_flag_and_refuses_an_agent_without_a_launch() {
        let store = store_with_ubuntu("check-run-mode");
        let probe = FakeProbe::new(ready("Ubuntu", 2));
        let mut ask = cli_ask("fake-cli");
        ask.headless = Headless::FromRunMode;
        assert_eq!(check_run_with(&store, &probe, &ask, r"D:\x").unwrap(), linux("Ubuntu", true));

        let mut shell = cli_ask("fake-cli");
        shell.kind = Kind::Shell;
        assert_eq!(check_run_with(&store, &probe, &shell, r"D:\x").unwrap_err().code, WSL_AGENT_NOT_SUPPORTED);
    }

    #[test]
    fn the_live_probe_is_the_process_wide_probe_cache() {
        let live = live_probe() as *const dyn PlaceProbe as *const ();
        let cache = crate::wsl::probe::cache() as *const crate::wsl::probe::LiveCache as *const ();
        assert_eq!(live, cache);
    }

    // ---- plan_with (the pane spawn) ----

    #[test]
    fn a_spawn_with_no_setting_plans_nothing_and_never_asks_the_probe() {
        let store = Store::at(&scratch("plan-none"));
        let probe = FakeProbe::new(ready("Ubuntu", 2));
        assert_eq!(plan_with(&store, &probe, &cli_ask("fake-cli"), None, r"D:\x").unwrap(), None);
        assert_eq!(probe.facts_calls.get(), 0);
    }

    #[test]
    fn a_spawn_in_linux_plans_the_place_from_the_setting() {
        let store = store_with_ubuntu("plan-setting");
        let probe = FakeProbe::new(ready("Ubuntu", 2));
        let place = plan_with(&store, &probe, &cli_ask("fake-cli"), None, r"D:\work\shop").unwrap().unwrap();
        assert_eq!(place.distro, "Ubuntu");
        assert_eq!(place.linux_cwd, "/mnt/d/work/shop");
        assert_eq!(place.program_abs, "/usr/bin/fake-cli");
    }

    #[test]
    fn a_one_shot_distro_beats_the_setting_and_a_refusal_comes_back_coded() {
        let store = Store::at(&scratch("plan-once"));
        let probe = FakeProbe::new(ready("Ubuntu", 2));
        let place = plan_with(&store, &probe, &cli_ask("fake-cli"), Some("Ubuntu"), r"D:\x").unwrap().unwrap();
        assert_eq!(place.linux_cwd, "/mnt/d/x");
        let mut ask = cli_ask("fake-cli");
        ask.account = Some("work");
        assert_eq!(
            plan_with(&store, &probe, &ask, Some("Ubuntu"), r"D:\x").unwrap_err().code,
            WSL_ACCOUNT_NOT_YET
        );
    }
}
