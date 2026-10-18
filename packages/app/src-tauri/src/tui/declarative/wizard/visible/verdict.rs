//! The AI look at a screen the wizard could not read ("Finish without me", SPEC 11.19).
//!
//! When the capture or the visible test reaches a "needs you" moment (an unsure ready screen, an unsure
//! reply, a question nobody recognises, a slow or stuck step), the front end sends the MASKED screen to the
//! drafter AI with a fixed small prompt and gets back one word out of `Verdict`. This file is the Rust side:
//! it takes that one word and decides, deterministically, what the wizard may do about it. The AI never types
//! anything and never chooses keys; the only things a verdict can cause are
//!
//! - `ready_for_input`: the same as the person's "It is ready" (bound to this exact screen, refused for a
//!   dialog or a screen that is working);
//! - `question_trust_or_once`: the SAFE ANSWERER (`answer.rs`) gets to look at the screen as a trust or
//!   one-time approval question. It still picks only a plain yes / trust this folder / allow once, never an
//!   "always", "all" or "don't ask again" option, and never a sign-in, payment, model or telemetry question;
//! - `working`, `question_other`, `unknown`: nothing is pressed; the front end waits, then skips the step;
//! - `sign_in_or_payment`: the run STOPS (nothing more is typed) with a plain message;
//! - `error`: the step is marked stuck so what was captured is kept, and the error screen goes to the drafter.

use super::screen::Flavour;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    ReadyForInput,
    Working,
    QuestionTrustOrOnce,
    QuestionOther,
    SignInOrPayment,
    Error,
    Unknown,
}

impl Verdict {
    pub const ALL: [Verdict; 7] = [
        Verdict::ReadyForInput,
        Verdict::Working,
        Verdict::QuestionTrustOrOnce,
        Verdict::QuestionOther,
        Verdict::SignInOrPayment,
        Verdict::Error,
        Verdict::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::ReadyForInput => "ready_for_input",
            Verdict::Working => "working",
            Verdict::QuestionTrustOrOnce => "question_trust_or_once",
            Verdict::QuestionOther => "question_other",
            Verdict::SignInOrPayment => "sign_in_or_payment",
            Verdict::Error => "error",
            Verdict::Unknown => "unknown",
        }
    }

    pub fn parse(text: &str) -> Option<Verdict> {
        let t = text.trim();
        Verdict::ALL.into_iter().find(|v| v.as_str() == t)
    }
}

/// What the wizard did about a verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// "It is ready" was pressed for this screen.
    PressedReady,
    /// The safe answerer looks at the screen as a trust or one-time approval question.
    SafeAnswerer,
    /// Nothing to do; keep waiting.
    Wait,
    /// Nothing may be pressed. The caller waits for a still screen once more, then skips the step.
    WaitThenSkip,
    /// The run stops. Nothing more is typed or pressed.
    Stop { code: &'static str },
    /// The step was marked stuck (what was captured is kept) because the agent shows an error.
    MarkedStuck,
    /// The visible test was stopped because the agent shows an error; the round fails and the screen goes to the drafter.
    StoppedTest,
    /// The verdict does not fit the screen (a dialog is up, nothing is drawn yet): nothing was done.
    Refused(String),
}

impl Decision {
    pub fn id(&self) -> &'static str {
        match self {
            Decision::PressedReady => "pressed_ready",
            Decision::SafeAnswerer => "safe_answerer",
            Decision::Wait => "wait",
            Decision::WaitThenSkip => "wait_then_skip",
            Decision::Stop { .. } => "stop",
            Decision::MarkedStuck => "marked_stuck",
            Decision::StoppedTest => "stopped_test",
            Decision::Refused(_) => "refused",
        }
    }

    pub fn stop_code(&self) -> Option<&'static str> {
        match self {
            Decision::Stop { code } => Some(code),
            _ => None,
        }
    }
}

/// `trust` for a question that mentions trusting a folder, project or workspace; otherwise a one-time approval.
pub fn flavour_of(screen: &str) -> Flavour {
    let lower = screen.to_lowercase();
    if lower.contains("trust") {
        Flavour::Trust
    } else {
        Flavour::Permission
    }
}

/// The words for a stop after a sign-in or payment screen.
pub fn sign_in_message(agent: &str) -> String {
    format!("Sign in to {agent} (or finish its setup), then press Retry.")
}

pub const SIGN_IN_CODE: &str = "WIZARD_SIGN_IN_NEEDED";
