//! Plain shells (`cmd`, `powershell`, `bash`): no agent, so "idle" means the prompt is showing.
//! Each shell has its own prompt shape in `prompt`; a standby shell is ready at its bare prompt.

mod prompt;

pub(crate) use prompt::host_prompt_visible;

use super::{Features, Kind, StandbyScreen, StandbySupport, TuiProfile};

const FEATURES: Features = Features {
    kind: Kind::Shell,
    dispatchable: false,
    read_only_headless: false,
    read_only_spawned_pane: false,
    resumable_headless: false,
};

/// A shell pane can be pre-launched and typed into (its prompt is its input box).
const STANDBY: StandbySupport = StandbySupport {
    visible: true,
    headless: false,
    experimental: false,
};

pub struct Cmd;

impl TuiProfile for Cmd {
    fn id(&self) -> &'static str {
        "cmd"
    }
    fn features(&self) -> Features {
        FEATURES
    }
    fn prompt_visible(&self, screen: &str) -> bool {
        prompt::cmd_visible(screen)
    }
    fn standby_support(&self) -> StandbySupport {
        STANDBY
    }
    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        prompt::cmd_ready(screen)
    }
}

pub struct Powershell;

impl TuiProfile for Powershell {
    fn id(&self) -> &'static str {
        "powershell"
    }
    fn features(&self) -> Features {
        FEATURES
    }
    fn prompt_visible(&self, screen: &str) -> bool {
        prompt::powershell_visible(screen)
    }
    fn standby_support(&self) -> StandbySupport {
        STANDBY
    }
    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        prompt::powershell_ready(screen)
    }
}

pub struct Bash;

impl TuiProfile for Bash {
    fn id(&self) -> &'static str {
        "bash"
    }
    fn features(&self) -> Features {
        FEATURES
    }
    fn prompt_visible(&self, screen: &str) -> bool {
        prompt::bash_visible(screen)
    }
    fn standby_support(&self) -> StandbySupport {
        STANDBY
    }
    fn standby_screen(&self, screen: &str) -> StandbyScreen {
        prompt::bash_ready(screen)
    }
}
