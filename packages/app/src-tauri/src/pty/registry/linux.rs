//! The Linux branch of `spawn_pane` (SPEC-n33 section 3.6 C): where a pane runs, and the `wsl.exe`
//! command for a pane that runs inside a distro. Generic: no agent is named here, the profile answers
//! `linux_launch`. When the place is Windows nothing in this file changes the spawn: `pane_command`
//! is then exactly `launch_command`.

use super::*;
use crate::operations::OperationError;
use crate::tui::{LaunchEnv, SpawnRequest, TuiProfile};
use crate::wsl::launch::{self, LaunchError, Target};
use crate::wsl::model::{LinuxPlace, WSL_ENV_REFUSED, WSL_NOT_INSTALLED, WSL_START_FAILED};
use crate::wsl::place::{self, Ask, Headless};
use std::path::Path;

/// A Linux pane whose process ends within this long failed to start: the cached facts of its distro
/// are dropped, so the next spawn probes again.
pub(super) const FAST_EXIT: Duration = Duration::from_secs(3);

/// Windows (`None`) or Linux (`Some`) for the pane about to spawn. Refusals come back as the person's
/// sentence. With no setting and no one-shot request this reads only the settings file.
pub(super) fn plan(
    profile: &dyn TuiProfile,
    pane_id: &str,
    account: Option<&str>,
    guard_env: Option<&str>,
    windows_cwd: &str,
) -> Result<Option<LinuxPlace>, String> {
    let ask = Ask {
        agent_type: profile.id(),
        pane_id,
        account,
        guard_env,
        hidden: crate::pty::hidden::is_hidden(pane_id),
        copy: false,
        headless: Headless::No,
        kind: profile.features().kind,
        linux_program: crate::tui::linux_program(profile.id()),
    };
    place::plan_spawn(&ask, windows_cwd).map_err(refusal)
}

/// The PTY command for a pane: today's `launch_command` for Windows, the `wsl.exe` command for Linux.
pub(super) fn pane_command(
    profile: &dyn TuiProfile,
    request: &SpawnRequest,
    env: &LaunchEnv,
    place: Option<&LinuxPlace>,
) -> Result<CommandBuilder, String> {
    let Some(place) = place else {
        return launch_command(profile, request, env);
    };
    let exe = crate::wsl::probe::wsl_exe()
        .ok_or_else(|| refusal(OperationError::new(WSL_NOT_INSTALLED, "WSL is not installed on this PC.", false)))?;
    linux_command(&exe, profile, request, env, place)
}

/// `wsl.exe -d <distro> --cd <folder> --exec /bin/sh -c <launcher> puppet-master <program> <args...>`,
/// in the pane's Windows folder. Only the names in `env` cross, and `WSLENV` is always set.
pub(super) fn linux_command(
    wsl_exe: &Path,
    profile: &dyn TuiProfile,
    request: &SpawnRequest,
    env: &LaunchEnv,
    place: &LinuxPlace,
) -> Result<CommandBuilder, String> {
    let linux_request = SpawnRequest {
        cwd: &place.linux_cwd,
        extra_args: request.extra_args,
        cols: request.cols,
        rows: request.rows,
        read_only: request.read_only,
    };
    let launch_spec = profile.linux_launch(&linux_request).ok_or_else(|| {
        refusal(OperationError::new(
            WSL_START_FAILED,
            format!(
                "{} can't run in Linux from Puppet Master yet.",
                crate::tui::descriptor_for(profile.id()).map(|d| d.label).unwrap_or_else(|| profile.id().to_string())
            ),
            false,
        ))
    })?;
    let target = Target { wsl_exe, place, windows_cwd: Path::new(request.cwd) };
    let spec = launch::wrap(&target, &launch_spec.args, env).map_err(|error| refusal(launch_refusal(error)))?;
    // The crossing env is already in the spec; nothing else is added on top of it.
    Ok(build_launch_spec_command(&spec, &LaunchEnv::default()))
}

/// Drops the cached facts of `distro` when the pane's process ends within `FAST_EXIT`. `exited` says
/// whether it ended; `look_every` is the poll step. Returns whether it was dropped.
pub(super) fn drop_facts_on_fast_exit(
    exited: &dyn Fn() -> bool,
    window: Duration,
    look_every: Duration,
    invalidate: &dyn Fn(),
) -> bool {
    let started = Instant::now();
    while started.elapsed() < window {
        if exited() {
            invalidate();
            return true;
        }
        thread::sleep(look_every);
    }
    false
}

/// Watches a new Linux pane for a fast exit (a thread that lives for `FAST_EXIT` at most).
pub(super) fn watch_fast_exit(distro: String, exited: Arc<Mutex<bool>>) {
    let _ = thread::Builder::new().name("wsl-fast-exit".into()).spawn(move || {
        drop_facts_on_fast_exit(
            &|| *exited.lock(),
            FAST_EXIT,
            Duration::from_millis(100),
            &|| crate::wsl::probe::invalidate(&distro),
        );
    });
}

fn launch_refusal(error: LaunchError) -> OperationError {
    match error {
        LaunchError::BadName(_) | LaunchError::WindowsPathValue(_) => OperationError::new(
            WSL_ENV_REFUSED,
            "A setting for this agent can't be passed into Linux.",
            false,
        ),
        LaunchError::BadDistro | LaunchError::NotAbsolute => {
            OperationError::new(WSL_START_FAILED, "Linux did not start: the place was not usable.", false)
        }
    }
}

/// The person's sentence for a refusal. The code goes to the log; the sentence is the error.
fn refusal(error: OperationError) -> String {
    tracing::warn!(code = %error.code, "linux pane refused");
    error.message
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod linux_tests;
