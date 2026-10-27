//! A person's message must never land on text the person typed in the terminal box. Delivery for a run clears
//! the box first; the outbox does not, so this wrapper looks at the box right before the message goes in and
//! refuses the write when it holds text. The refusal is how delivery stops without typing a letter.

use crate::agent_runs::delivery::DeliveryIo;
use crate::agent_runs::readiness::{self, Observation};
use crate::pty::WriteOrigin;
use crate::tui::{ComposerState, TuiProfile};
use std::sync::Arc;
use std::time::Duration;

/// How the write error that means "the box holds text" starts.
pub const DRAFT_PREFIX: &str = "DRAFT_PRESENT";
const DRAFT_REFUSED: &str = "DRAFT_PRESENT: the terminal box holds text that was not typed by the outbox";

pub struct DraftGuard<'a> {
    inner: &'a mut dyn DeliveryIo,
    /// The message text went in once: from here the box holds our own text.
    typed: bool,
}

impl<'a> DraftGuard<'a> {
    pub fn new(inner: &'a mut dyn DeliveryIo) -> Self {
        Self { inner, typed: false }
    }
}

impl DeliveryIo for DraftGuard<'_> {
    fn write(&mut self, origin: WriteOrigin<'_>, text: &str, enter: bool) -> Result<(), String> {
        // Only the first text of the message is checked; Enter and later keys are for our own text.
        let first_text = !self.typed && !text.is_empty() && matches!(origin, WriteOrigin::Task(_));
        if first_text {
            let profile = self.inner.profile();
            let verdict = readiness::assess(profile.as_ref(), self.inner.observe());
            if verdict.composer() == ComposerState::HasText {
                return Err(DRAFT_REFUSED.to_string());
            }
        }
        let wrote = self.inner.write(origin, text, enter);
        // A refused or stale write typed nothing, so the next try is checked again.
        if first_text && wrote.is_ok() {
            self.typed = true;
        }
        wrote
    }

    fn screen(&mut self) -> String {
        self.inner.screen()
    }

    fn observe(&mut self) -> Observation {
        self.inner.observe()
    }

    fn busy(&mut self) -> bool {
        self.inner.busy()
    }

    fn pause(&mut self, duration: Duration) {
        self.inner.pause(duration)
    }

    fn enter_settle(&mut self) -> Duration {
        self.inner.enter_settle()
    }

    fn profile(&mut self) -> Arc<dyn TuiProfile> {
        self.inner.profile()
    }
}
