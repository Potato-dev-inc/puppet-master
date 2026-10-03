//! Plain shells (`cmd`, `powershell`, `bash`): no agent, so "idle" means the prompt is showing.
//! Each shell has its own prompt shape in `prompt`.

mod prompt;

use super::{Features, Kind, TuiProfile};

const FEATURES: Features = Features {
    kind: Kind::Shell,
    dispatchable: false,
    read_only_headless: false,
    resumable_headless: false,
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
}
