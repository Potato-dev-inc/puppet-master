//! The env a guarded launch gets, resolved for the real app: the guard name must be known and the
//! empty `GH_CONFIG_DIR` folder must exist. Shared by the pane spawn and the headless launch, so
//! both fail closed the same way (an unknown guard name, or a folder that cannot be made, refuses
//! the launch; nothing starts unguarded).

use crate::tui::LaunchEnv;
use std::path::PathBuf;

/// The private empty folder the guard points `GH_CONFIG_DIR` at.
#[cfg(not(test))]
fn empty_gh_dir() -> Result<PathBuf, String> {
    crate::issue_intake::store::Store::live().gh_empty_dir()
}

/// Tests never touch the app's own data folder.
#[cfg(test)]
fn empty_gh_dir() -> Result<PathBuf, String> {
    crate::issue_intake::store::Store::at(std::env::temp_dir().join(format!("pm-guard-launch-{}", std::process::id())))
        .gh_empty_dir()
}

/// The guard's launch env, or the reason the launch is refused.
pub fn launch_env_for(guard: &str) -> Result<LaunchEnv, String> {
    if !super::env_guard::known(guard) {
        return Err(format!("unknown env guard {guard:?}; the launch is refused"));
    }
    let dir = empty_gh_dir().map_err(|err| format!("env guard {guard}: {err}"))?;
    super::env_guard::launch_env(guard, &dir).ok_or_else(|| format!("unknown env guard {guard:?}; the launch is refused"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_guard_names_are_refused_not_dropped() {
        for name in ["", "nope", "NO_PUSH", "no-push", " no_push"] {
            let err = launch_env_for(name).unwrap_err();
            assert!(err.contains("refused"), "{name}: {err}");
        }
    }

    #[test]
    fn the_known_guard_points_gh_at_an_existing_empty_folder() {
        let env = launch_env_for(super::super::env_guard::NO_PUSH).expect("guard");
        let dir = env.set.iter().find(|(k, _)| k == "GH_CONFIG_DIR").map(|(_, v)| PathBuf::from(v)).expect("dir");
        assert!(dir.is_dir(), "{}", dir.display());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the folder stays empty");
    }
}
