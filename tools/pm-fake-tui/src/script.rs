//! The script a fake agent plays: TOML (or JSON, by file extension).
//!
//! ```toml
//! agent = "claude"                 # which layout to draw: "claude" or "codex"
//! boot_ms = 300                    # time on the loading screen before anything else
//!
//! [[startup]]                      # screens shown before the composer takes input, in order
//! gate = "trust"                   # claude/codex folder trust, or codex's "update_menu"
//! after_ms = 100
//!
//! [[turn]]
//! on_prompt = "hello"              # substring of the submitted prompt (case-insensitive); omit = any prompt
//! steps = [
//!   { do = "think", ms = 600 },
//!   { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "say", text = "tests pass" }] },
//!   { do = "write_file", path = "out.txt", content = "hi" },
//!   { do = "say", text = "done" },
//!   { do = "suggest", text = "commit this" },
//! ]
//! ```
//!
//! Every step is `{ do = "<name>", ... }`. A turn that runs out of steps ends the way a real turn does
//! (a summary row, then the composer). `exit`, `crash` and `limit` end the process or the turn early.

use serde::Deserialize;

/// Which agent's layout to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    #[default]
    Claude,
    Codex,
    /// An agent no profile knows (the Agent Wizard's end-to-end tests): a boxed `> ` input row, a braille
    /// spinner with `ctrl+c to stop`, `reply:` rows and `Allow ...? [y/n]` questions.
    Nimbus,
}

/// What a too-quick Enter does (the paste-burst quirk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BurstEffect {
    /// The Enter becomes a line break in the composer (Codex).
    Newline,
    /// The Enter is dropped (Claude 2.1.287: "review and press Enter to send").
    Drop,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default)]
    pub agent: Layout,
    /// Version shown in the banner (default: the version the fixtures were captured from).
    #[serde(default)]
    pub version: Option<String>,
    /// Model line of the Claude banner / the Codex footer.
    #[serde(default)]
    pub model: Option<String>,
    /// Folder shown in the banner (default: the process's working directory).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Time on the loading screen before the first startup screen or the composer.
    #[serde(default)]
    pub boot_ms: u64,
    /// An Enter arriving within this many ms of the last typed character is part of a paste, not a
    /// submit. Default: 120 for Codex (its real window), 0 for Claude.
    #[serde(default)]
    pub paste_burst_ms: Option<u64>,
    #[serde(default)]
    pub paste_burst_effect: Option<BurstEffect>,
    /// Esc during a turn interrupts it (default true). False: the agent ignores Esc.
    #[serde(default = "yes")]
    pub interrupt: bool,
    /// Where to append a JSON line per event (prompt received, key answered, ...). The env var
    /// `PM_FAKE_TUI_LOG` wins.
    #[serde(default)]
    pub log: Option<String>,
    #[serde(default)]
    pub startup: Vec<Gate>,
    #[serde(default)]
    pub turn: Vec<Turn>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// The folder-trust question. Claude: `No, exit` / `Yes, I trust this folder`. Codex: `1. Yes, continue` / `2. No, quit`.
    /// Nimbus: `Do you trust the files in this folder?` with `1. Yes, trust this folder` / `2. No, exit`.
    Trust,
    /// Codex's "Update available" menu: Update now / Skip / Skip until next version.
    UpdateMenu,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub gate: GateKind,
    /// How long after the previous screen (or the end of `boot_ms`) this one appears.
    #[serde(default)]
    pub after_ms: u64,
    /// Draw the composer while waiting for `after_ms`: the race where the menu opens OVER a composer
    /// that already looked ready (live: Codex 0.159.3, the menu came up 0-600 ms after start).
    #[serde(default)]
    pub composer_first: bool,
    /// False: the screen ignores Esc (a menu that does not close), so a run can only park.
    #[serde(default = "yes")]
    pub honour_esc: bool,
    /// After the screen is passed (Trust, Skip), the agent keeps loading this long: its composer is
    /// drawn and looks ready, but typed keys are held and only taken, all at once and in order, when
    /// the time is up (live v0.2.0 hunt, Codex 0.162.1 after Trust: the task typed into the drawn
    /// composer showed up seconds later).
    #[serde(default)]
    pub warm_ms: u64,
    /// The screen stays drawn this long after the answering key (the agent redraws late): keys
    /// that arrive meanwhile are held and reach whatever comes next (live v0.2.0 hunt: five Trust
    /// answers in 400 ms all found the dialog still drawn).
    #[serde(default)]
    pub linger_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    /// Substring of the submitted prompt, case-insensitive. Omitted or `"*"`: any prompt.
    #[serde(default)]
    pub on_prompt: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// "Bash command ... Do you want to proceed?"
    Bash,
    /// "Edit file ... Do you want to make this edit to <path>?"
    Edit,
    /// "Create file ... Do you want to create <path>?"
    Create,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Show the working row for `ms` (the agent is thinking).
    Think {
        ms: u64,
        #[serde(default)]
        verb: Option<String>,
    },
    /// Stay busy for `ms` without changing the screen's content.
    Wait { ms: u64 },
    /// Add a reply paragraph.
    Say { text: String },
    /// Add a tool call row, optionally failed (`failed = -1` draws `Failed (exit -1)` on Codex).
    Tool {
        label: String,
        #[serde(default)]
        detail: Option<String>,
        #[serde(default)]
        ms: u64,
        #[serde(default)]
        failed: Option<i32>,
    },
    /// Ask for permission and wait for the answer. `on_allow` runs after a yes (option 1 or 2),
    /// `on_deny` after a no; a no then ends the turn (the rest of the steps are dropped).
    Approve {
        kind: ApprovalKind,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        path: Option<String>,
        /// Rows of the diff shown for an edit.
        #[serde(default)]
        diff: Vec<String>,
        /// Claude's extra "Yes, and switch to auto mode" row (a four-option menu).
        #[serde(default)]
        auto_mode_option: bool,
        #[serde(default)]
        on_allow: Vec<Step>,
        #[serde(default)]
        on_deny: Vec<Step>,
    },
    /// Write a file under the working directory (as the agent would, after it was allowed to).
    WriteFile {
        path: String,
        content: String,
        #[serde(default)]
        append: bool,
        /// Do not draw the "Write(path)" rows.
        #[serde(default)]
        quiet: bool,
    },
    /// After the turn, show this as the dimmed suggestion in the composer.
    Suggest { text: String },
    /// Hit the usage limit: print the vendor's message and end the turn. With `auto_continue` the
    /// agent then waits for the reset ("continuing automatically ... esc to cancel").
    Limit {
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        reset: Option<String>,
        #[serde(default)]
        auto_continue: bool,
    },
    /// Exit the process, leaving the screen as it is (the host shell's prompt appears under it).
    Exit {
        #[serde(default)]
        code: i32,
    },
    /// Die: print a panic line and exit with a failure code.
    Crash {
        #[serde(default)]
        message: Option<String>,
        #[serde(default = "crash_code")]
        code: i32,
    },
    /// Add literal rows to the transcript (a captured screen, a banner). No styling.
    Screen { text: String },
}

fn crash_code() -> i32 {
    101
}

impl Script {
    pub fn parse(text: &str, json: bool) -> Result<Script, String> {
        let script: Script = if json {
            serde_json::from_str(text).map_err(|e| format!("script is not valid JSON: {e}"))?
        } else {
            toml::from_str(text).map_err(|e| format!("script is not valid TOML: {e}"))?
        };
        script.validate()?;
        Ok(script)
    }

    pub fn load(path: &std::path::Path) -> Result<Script, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let json = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        Script::parse(&text, json)
    }

    fn validate(&self) -> Result<(), String> {
        fn check(steps: &[Step]) -> Result<(), String> {
            for step in steps {
                if let Step::Approve { kind, command, path, on_allow, on_deny, .. } = step {
                    match kind {
                        ApprovalKind::Bash if command.as_deref().is_none_or(str::is_empty) => {
                            return Err("an `approve` of kind \"bash\" needs a `command`".into())
                        }
                        ApprovalKind::Edit | ApprovalKind::Create if path.as_deref().is_none_or(str::is_empty) => {
                            return Err("an `approve` of kind \"edit\" or \"create\" needs a `path`".into())
                        }
                        _ => {}
                    }
                    check(on_allow)?;
                    check(on_deny)?;
                }
            }
            Ok(())
        }
        for turn in &self.turn {
            check(&turn.steps)?;
        }
        Ok(())
    }

    /// The turn to play for a submitted prompt: the first whose `on_prompt` is inside it, else the
    /// first without one.
    pub fn turn_for(&self, prompt: &str) -> Option<&Turn> {
        let lower = prompt.to_lowercase();
        self.turn
            .iter()
            .find(|turn| {
                turn.on_prompt
                    .as_deref()
                    .is_some_and(|needle| needle != "*" && lower.contains(&needle.to_lowercase()))
            })
            .or_else(|| {
                self.turn
                    .iter()
                    .find(|turn| turn.on_prompt.as_deref().is_none_or(|needle| needle == "*"))
            })
    }

    pub fn burst_ms(&self) -> u64 {
        self.paste_burst_ms.unwrap_or(match self.agent {
            Layout::Codex => 120,
            Layout::Claude | Layout::Nimbus => 0,
        })
    }

    pub fn burst_effect(&self) -> BurstEffect {
        self.paste_burst_effect.unwrap_or(match self.agent {
            Layout::Codex => BurstEffect::Newline,
            Layout::Claude | Layout::Nimbus => BurstEffect::Drop,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_script_parses_and_picks_turns() {
        let script = Script::parse(
            r#"
            agent = "codex"
            boot_ms = 50
            [[startup]]
            gate = "update_menu"
            after_ms = 600
            composer_first = true
            [[turn]]
            on_prompt = "approve"
            steps = [
              { do = "think", ms = 10 },
              { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "say", text = "ok" }], on_deny = [{ do = "say", text = "no" }] },
              { do = "suggest", text = "commit this" },
            ]
            [[turn]]
            steps = [{ do = "say", text = "fallback" }]
            "#,
            false,
        )
        .unwrap();
        assert_eq!(script.agent, Layout::Codex);
        assert_eq!(script.startup[0].gate, GateKind::UpdateMenu);
        assert!(script.startup[0].composer_first && script.startup[0].honour_esc);
        assert_eq!(script.turn_for("please APPROVE this").unwrap().steps.len(), 3);
        assert_eq!(script.turn_for("something else").unwrap().steps.len(), 1);
        assert_eq!((script.burst_ms(), script.burst_effect()), (120, BurstEffect::Newline));
    }

    #[test]
    fn json_scripts_work_too() {
        let script = Script::parse(r#"{"agent":"claude","turn":[{"steps":[{"do":"exit","code":3}]}]}"#, true).unwrap();
        assert!(matches!(script.turn[0].steps[0], Step::Exit { code: 3 }));
        assert_eq!(script.burst_ms(), 0);
    }

    #[test]
    fn mistakes_are_named() {
        assert!(Script::parse("agnet = \"claude\"", false).unwrap_err().contains("agnet"));
        let err = Script::parse("[[turn]]\nsteps = [{ do = \"approve\", kind = \"bash\" }]", false).unwrap_err();
        assert!(err.contains("command"), "{err}");
        let err = Script::parse("[[turn]]\nsteps = [{ do = \"dance\" }]", false).unwrap_err();
        assert!(err.contains("dance") || err.contains("variant"), "{err}");
    }
}
