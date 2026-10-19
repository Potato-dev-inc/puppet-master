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
    /// Nimbus only: this many filler rows of an "earlier session" sit under the banner from the start, so the
    /// banner scrolls off the top and a long conversation overflows the screen.
    #[serde(default)]
    pub backlog_rows: usize,
    /// Nimbus only: literal rows printed under the banner row (before the blank row), like the logo and the
    /// "<Name> CLI <version>" line an inline TUI prints first (`["▄▀ Nimbus CLI 1.4.2.", "Signed in as dev@example.com"]`).
    #[serde(default)]
    pub banner_rows: Vec<String>,
    /// Nimbus only: how permission questions look. `yn`: one line `Allow nimbus to ...? [y/n]` for everything.
    /// `menus`: a command asks with a numbered menu (`1. Allow once` / `2. Always allow ...` / `3. Deny`, digits answer),
    /// a file write asks with an arrow menu (`Yes, write it` ... , up/down and enter), two different questions.
    /// The styled shapes (the same question for a command and a file, SPEC 11.20): `inverse` (an arrow menu whose
    /// highlighted row is drawn in inverse video and has NO marker), `boxed` (the arrow menu inside a box, `| > Allow once |`),
    /// `buttons` (one row `Allow once  Always allow  Deny`, the selected button in inverse video, moved with Tab only),
    /// `buttons_arrows` (the same row moved with Left / Right only), `inline` (`... [Y]es / [N]o`, the letters answer) and
    /// `always_only` (an inverse menu whose only yes is `Always allow in this project`).
    #[serde(default)]
    pub approvals: Approvals,
    /// Nimbus only: which option (0-based) the cursor of a permission question starts on (default 0, the first).
    #[serde(default)]
    pub approval_cursor: usize,
    /// Nimbus only: a model name shown in a status line (`model: <name>`), separate from the banner's.
    #[serde(default)]
    pub status_model: Option<String>,
    /// When the status line shows it: in the footer all the time (`always`, default) or on the working row only (`busy`).
    #[serde(default)]
    pub status_model_when: StatusWhen,
    /// Nimbus only: the model name the status line (footer, working row) shows is cut to this many characters, like a
    /// narrow status bar that clips its right end. The banner keeps the whole name.
    #[serde(default)]
    pub status_model_clip: Option<usize>,
    /// Nimbus only: the row printed after every turn, instead of `done in {secs}s`. `{secs}` is the turn's length.
    /// A post-turn summary such as `▸ Thought for {secs}s, 298 tokens`.
    #[serde(default)]
    pub summary_row: Option<String>,
    /// Nimbus only: `false` draws no `> <task>` row for the submitted task (an agent that does not echo what it was
    /// asked). The default is to echo it.
    #[serde(default = "yes")]
    pub echo_task: bool,
    /// Nimbus only: text drawn flush against the right edge of the footer row (a clipped column shows at once).
    #[serde(default)]
    pub footer_right: Option<String>,
    /// Nimbus only: paint like an inline TUI. No cell addressing from the top of the screen: the conversation
    /// is printed top-down into the scrollback, only the bottom of it (the part still on screen) is redrawn.
    #[serde(default)]
    pub inline: bool,
    /// Nimbus inline only: a resize prints the WHOLE conversation again (the screen is cleared first, the rows
    /// that overflow it scroll into the history a second time). False: only the visible part is redrawn.
    #[serde(default)]
    pub reprint_on_resize: bool,
    #[serde(default)]
    pub startup: Vec<Gate>,
    #[serde(default)]
    pub turn: Vec<Turn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Approvals {
    #[default]
    Yn,
    Menus,
    Inverse,
    Boxed,
    Buttons,
    ButtonsArrows,
    Inline,
    AlwaysOnly,
    /// An arrow menu whose cursor cannot be seen at all (no marker, no colour): only the person's eyes on the real
    /// program would know where it is. The parser cannot read it; an AI look names the option.
    Blind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StatusWhen {
    #[default]
    Always,
    Busy,
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

/// How the nimbus layout draws its folder-trust question (the Agent Wizard e2e tests answer each of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrustStyle {
    /// `> 1. Yes, trust this folder` / `  2. No, exit`; the digits answer at once.
    #[default]
    Numbered,
    /// An arrow menu with the cursor on the safe row, like Antigravity CLI:
    /// `> Yes, I trust this folder` / `  No, exit` and `up/down Navigate - enter Confirm`. Digits do nothing.
    Arrow,
    /// The same menu with the cursor on `No, exit` and the safe row second.
    ArrowNoFirst,
    /// An arrow menu whose only yes is `Yes, always trust this folder`.
    Always,
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
    /// Nimbus only: how the trust question looks.
    #[serde(default)]
    pub style: TrustStyle,
    /// Nimbus only: the screen keeps redrawing while it waits (a spinner row with a seconds counter under the
    /// question and a blinking block after it), like an agent that shows it is alive.
    #[serde(default)]
    pub animate: bool,
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
    /// Add a reply of `count` rows, one per line, built from `template` (`{n}` is the row number, 1-based): a reply
    /// taller than the pane.
    Lines {
        count: usize,
        #[serde(default)]
        template: Option<String>,
    },
    /// Add a reply row that holds an OSC 8 hyperlink: `before`, then `text` as the link's visible text pointing at
    /// `target` (a `file://` URL, say), then `after`. Nimbus draws it as a `reply:` row.
    Link {
        text: String,
        target: String,
        #[serde(default)]
        before: String,
        #[serde(default)]
        after: String,
    },
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
    /// Run a shell command in the working directory (as the agent's shell tool would, after it was
    /// allowed to): `cmd /C` on Windows, `sh -c` elsewhere. Draws a tool row with the command and
    /// whether it exited 0; the exit code and the last output lines go to the log. The turn goes on
    /// whatever the exit code (a script that must stop on a failure follows with a `crash`).
    Run { command: String },
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
    /// Show these rows and stop: no composer, no spinner, nothing moving, until a key arrives (then the
    /// turn ends). A menu or question no profile has ever seen; only the nimbus layout draws it.
    Hold { text: String },
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
