//! Per-run supervisor thread: polls a run's snapshot, observes its visible pane (or the native
//! OpenCode session), settles the turn when the pane shows a decision, and enforces the deadline.
//!
//! One tick is `observe_pane` (what the pane says) followed by `enforce_deadline`. Each decision
//! the completion watch can reach has its own small handler returning whether it settled the run.

use super::pane_turn::{
    auto_approve_allowed_command, not_captured_diagnostics, previous_turn_task, read_tui_screen,
    record_completion_model, redeliver_prompt, redelivery_dead_end,
};
use crate::agent_runs::completion::{self, TuiTurnDecision, TuiTurnWatch};
use crate::agent_runs::{native, runtime, transcript};
use crate::operations::{
    self, OperationError, OperationSnapshot, OperationStatus, ResultCapture, StateSource,
};
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Consecutive failed snapshot reads (500 ms apart) the deadline supervisor tolerates.
const SUPERVISOR_MAX_READ_FAILURES: u32 = 40;
/// Supervisor tick. Was 1000 ms for opencode_native, which put up to a second of dead time after
/// the model finished into every run.
const SUPERVISE_POLL_MS: u64 = 250;
/// Re-reads of the pane before a turn is declared not captured, and the pause between them.
const REREAD_ATTEMPTS: u32 = 2;
const REREAD_DELAY_MS: u64 = 1500;

/// What one pane observation tells the supervisor loop to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Nothing settled the run; go on to the deadline check.
    Continue,
    /// The run reached a terminal state; the supervisor is done.
    Finished,
    /// The prompt was re-delivered (or tried); the next tick observes the pane again.
    Redelivered,
}

/// Starts the supervisor thread for a run already created and dispatched.
pub fn supervise_existing(
    snapshot: OperationSnapshot,
    registry: Arc<Mutex<PaneRegistry>>,
    app: tauri::AppHandle,
) {
    let _ = thread::Builder::new()
        .name("puppet-agent-deadline".into())
        .spawn(move || Supervisor::new(&snapshot, registry, app).run(&snapshot));
}

struct Supervisor {
    registry: Arc<Mutex<PaneRegistry>>,
    app: tauri::AppHandle,
    watch: TuiTurnWatch,
    /// One automatic re-delivery per turn when the prompt never started a turn.
    redelivered: bool,
}

impl Supervisor {
    fn new(
        snapshot: &OperationSnapshot,
        registry: Arc<Mutex<PaneRegistry>>,
        app: tauri::AppHandle,
    ) -> Self {
        let watch =
            TuiTurnWatch::from_baseline(snapshot.output_baseline.clone().unwrap_or_default())
                .with_task(snapshot.task.clone())
                .with_previous_task(previous_turn_task(snapshot))
                .with_verified_delivery(verifies_delivery(&snapshot.agent_type));
        Self {
            registry,
            app,
            watch,
            redelivered: false,
        }
    }

    fn run(mut self, snapshot: &OperationSnapshot) {
        let poll = Duration::from_millis(SUPERVISE_POLL_MS);
        if let Some(expired) = supervise_loop(snapshot, poll, &mut |current| self.tick(current)) {
            self.enforce_deadline(&expired);
        }
    }

    /// One observation of a live run: its baseline, then what its pane (or native session) says.
    fn tick(&mut self, current: &OperationSnapshot) -> Step {
        self.capture_baseline(current);
        if current.agent_type == "opencode_native" {
            native::supervise_native_observation(current, &self.registry, &self.app);
        } else if matches!(
            current.status,
            OperationStatus::Running | OperationStatus::WaitingInput
        ) {
            if let Some(pane_id) = current.pane_id.as_deref() {
                return self.observe_pane(current, pane_id);
            }
        }
        Step::Continue
    }

    /// Before the turn starts, the screen is the baseline that its output is measured against.
    fn capture_baseline(&mut self, current: &OperationSnapshot) {
        if !self.watch.baseline.is_empty() {
            return;
        }
        let Some(pane_id) = current.pane_id.as_deref() else {
            return;
        };
        let pre_turn = current.status != OperationStatus::Running
            || current.stage.as_deref() == Some("waiting_for_pane_readiness");
        if pre_turn {
            if let Some(screen) = read_tui_screen(&self.registry, pane_id) {
                self.watch.baseline = screen;
            }
        }
    }

    fn publish(&self, snapshot: &OperationSnapshot) {
        let _ = crate::bridge::publish_operation(snapshot, &self.registry, &self.app);
    }

    /// Marks the turn finished and publishes it. False when the state write failed.
    fn finish_run(
        &self,
        current: &OperationSnapshot,
        result: Option<String>,
        capture: ResultCapture,
    ) -> bool {
        match operations::mark_operation_finished(
            &current.project_path,
            &current.operation_id,
            result,
            capture,
            false,
        ) {
            Ok(completed) => {
                self.publish(&completed);
                true
            }
            Err(_) => false,
        }
    }

    /// Marks the turn failed with an inferred `failed` stage. False when the state write failed.
    fn fail_run(&self, current: &OperationSnapshot, error: OperationError) -> bool {
        match operations::mark_operation_state(
            &current.project_path,
            &current.operation_id,
            OperationStatus::Failed,
            StateSource::Inferred,
            Some("failed".into()),
            None,
            Some(error),
        ) {
            Ok(failed) => {
                self.publish(&failed);
                true
            }
            Err(_) => false,
        }
    }

    /// Reads the pane of a running/waiting turn and acts on what the completion watch decides.
    fn observe_pane(&mut self, current: &OperationSnapshot, pane_id: &str) -> Step {
        let pane_status = self
            .registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == pane_id)
            .map(|pane| pane.status)
            .unwrap_or_default();
        let action = native::infer_tui_prompt(&self.registry, pane_id, &current.agent_type);
        let has_prompt = action.is_some() || pane_status == "waiting_input";
        // The run left `starting` without its prompt ever being typed (the dispatch was
        // abandoned: see `operations::prompt_dispatched`). Watching the pane for a reply to a
        // prompt the model never received ends in RESULT_NOT_CAPTURED, so deliver it now, once,
        // through the verified protocol.
        if crate::agent_runs::delivery::needs_redelivery(
            verifies_delivery(&current.agent_type),
            current.status == OperationStatus::Running,
            operations::dispatch_in_flight(&current.operation_id),
            operations::prompt_dispatched(current),
            self.redelivered,
        ) && !has_prompt
        {
            // The pane will never take the prompt (the user declined the startup dialog, or the
            // agent is gone and a shell prompt shows): end the run with the reason instead of
            // asking again every tick.
            let screen = read_tui_screen(&self.registry, pane_id).unwrap_or_default();
            if let Some(error) = redelivery_dead_end(&current.agent_type, pane_id, &screen) {
                transcript::append(
                    &current.project_path,
                    &current.operation_id,
                    "diagnostic",
                    &format!("{}: {}", error.code, error.message),
                );
                if self.fail_run(current, error) {
                    return Step::Finished;
                }
                return Step::Continue;
            }
            if redeliver_prompt(
                current,
                &self.registry,
                &self.app,
                &mut self.watch,
                "the run left starting before its prompt was dispatched",
            ) {
                self.redelivered = true;
            }
            return Step::Redelivered;
        }
        let screen = read_tui_screen(&self.registry, pane_id).unwrap_or_default();
        self.watch.set_delivery_verified(
            verifies_delivery(&current.agent_type) && operations::prompt_dispatched(current),
        );
        let settled = match completion::observe_tui_turn(
            &mut self.watch,
            &pane_status,
            &screen,
            has_prompt,
            &current.agent_type,
        ) {
            TuiTurnDecision::WaitingInput => {
                self.on_waiting_input(current, action, &screen);
                false
            }
            TuiTurnDecision::Complete { result, capture } => {
                self.on_complete(current, pane_id, &screen, result, capture)
            }
            TuiTurnDecision::AmbiguousIdle if current.checks.is_empty() => {
                self.on_idle_without_reply(current, pane_id)
            }
            TuiTurnDecision::AmbiguousIdle => self.on_idle_with_checks(current, pane_id),
            TuiTurnDecision::RetrySubmit => {
                transcript::append(
                    &current.project_path,
                    &current.operation_id,
                    "delivery",
                    "pane idle with no activity for 3s; Enter sent once more",
                );
                // Empty text + newline writes just Enter.
                let _ = crate::pty::registry_write_input(
                    &self.registry,
                    &self.app,
                    pane_id,
                    "",
                    true,
                    false,
                    None,
                );
                false
            }
            TuiTurnDecision::NotSubmitted
                if !self.redelivered && verifies_delivery(&current.agent_type) =>
            {
                // The turn never started. Re-run the delivery protocol once
                // (clear, retype, verify, submit) before failing the run.
                self.redelivered = true;
                redeliver_prompt(
                    current,
                    &self.registry,
                    &self.app,
                    &mut self.watch,
                    "prompt not submitted",
                );
                false
            }
            TuiTurnDecision::NotSubmitted => self.on_not_submitted(current, pane_id),
            TuiTurnDecision::Continue => false,
        };
        if settled {
            Step::Finished
        } else {
            Step::Continue
        }
    }

    /// A permission/question is on screen: auto-approve it when the run's policy allows, else park
    /// the run in `waiting_input` with the prompt as its required action.
    fn on_waiting_input(
        &self,
        current: &OperationSnapshot,
        action: Option<serde_json::Value>,
        screen: &str,
    ) {
        let approved = action.as_ref().is_some_and(|action| {
            auto_approve_allowed_command(current, action, screen, &self.registry, &self.app)
        });
        let Some(action) = action.filter(|_| !approved) else {
            return;
        };
        if current.status == OperationStatus::WaitingInput
            && current.required_action.as_ref() == Some(&action)
        {
            return;
        }
        transcript::append(
            &current.project_path,
            &current.operation_id,
            "permission",
            action
                .get("summary")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("permission required"),
        );
        if let Ok(waiting) = operations::mark_operation_waiting_input(
            &current.project_path,
            &current.operation_id,
            Some("waiting_input".into()),
            action,
            StateSource::Inferred,
        ) {
            self.publish(&waiting);
        }
    }

    /// The watch found the turn's reply: widen it from the scrollback if needed and finish.
    fn on_complete(
        &self,
        current: &OperationSnapshot,
        pane_id: &str,
        screen: &str,
        result: String,
        capture: ResultCapture,
    ) -> bool {
        record_completion_model(current, screen);
        // The reply was read from the visible rows; one longer than the screen is re-read from
        // the scrollback, or flagged as truncated.
        let widened = match crate::pty::registry_read_full_text(self.registry.as_ref(), pane_id) {
            Ok(full) => completion::widen_reply(
                screen,
                &full,
                &current.task,
                &previous_turn_task(current),
                &result,
                &current.agent_type,
            ),
            Err(_) => completion::WidenedReply {
                text: result.clone(),
                truncated: false,
            },
        };
        let result = widened.text;
        if widened.truncated {
            let _ = operations::add_warning(
                &current.project_path,
                &current.operation_id,
                runtime::RESULT_TRUNCATED,
                "the reply is longer than the pane's visible rows and its start could not be found in the scrollback; result holds only the last part of it. Read the pane or agent_transcript for the rest",
            );
        }
        transcript::append(
            &current.project_path,
            &current.operation_id,
            "assistant_final",
            &result,
        );
        self.finish_run(current, Some(result), capture)
    }

    /// The pane went idle with no reply the watch could read and the run has no checks to decide
    /// it: re-read the pane a bounded number of times, then fail with the evidence.
    fn on_idle_without_reply(&self, current: &OperationSnapshot, pane_id: &str) -> bool {
        // Bounded re-read before giving up: the screen may have finished painting, or the reply
        // may sit in the scrollback.
        let mut recovered = None;
        for attempt in 0..REREAD_ATTEMPTS {
            recovered = self.recapture(pane_id, &current.agent_type);
            if recovered.is_some() || attempt + 1 == REREAD_ATTEMPTS {
                break;
            }
            thread::sleep(Duration::from_millis(REREAD_DELAY_MS));
        }
        if let Some(result) = recovered {
            transcript::append(
                &current.project_path,
                &current.operation_id,
                "delivery",
                "reply was not on the visible rows when the pane went idle; recovered by re-reading the pane",
            );
            transcript::append(
                &current.project_path,
                &current.operation_id,
                "assistant_final",
                &result,
            );
            if self.finish_run(current, Some(result), ResultCapture::Inferred) {
                return true;
            }
        }
        let mut error = OperationError::new(
            "RESULT_NOT_CAPTURED",
            "the pane went idle but no reply text could be captured for this turn; read the pane or agent_transcript to see what happened",
            true,
        );
        error.context = not_captured_diagnostics(&self.registry, pane_id, &self.watch);
        transcript::append(
            &current.project_path,
            &current.operation_id,
            "diagnostic",
            &error.context.to_string(),
        );
        self.fail_run(current, error)
    }

    /// Same idle-without-reply situation, but the run's checks decide it, so a missing reply is
    /// not hidden: say what the pane showed (the same evidence a RESULT_NOT_CAPTURED failure
    /// carries) and that nothing was inferred.
    fn on_idle_with_checks(&self, current: &OperationSnapshot, pane_id: &str) -> bool {
        if let Some(result) = self.recapture(pane_id, &current.agent_type) {
            transcript::append(
                &current.project_path,
                &current.operation_id,
                "assistant_final",
                &result,
            );
            if self.finish_run(current, Some(result), ResultCapture::Inferred) {
                return true;
            }
        }
        let diagnostics = not_captured_diagnostics(&self.registry, pane_id, &self.watch);
        transcript::append(
            &current.project_path,
            &current.operation_id,
            "diagnostic",
            &diagnostics.to_string(),
        );
        let _ = operations::add_warning(
            &current.project_path,
            &current.operation_id,
            runtime::NO_REPLY_TEXT,
            "the turn finished but no reply text could be read from the pane under the prompt, so there is nothing to check. If the pane shows the prompt and an empty reply, the model itself answered with an empty message (seen with Cursor on bare-data replies such as [] or {} on a follow-up, confirmed in its own chat store). No result was inferred; the pane evidence is in agent_transcript",
        );
        self.finish_run(current, None, ResultCapture::Missing)
    }

    /// The pane stayed idle and empty: the prompt was probably never submitted. Clear the input
    /// box (the unsent prompt is still typed there) and fail the run as safely retryable.
    fn on_not_submitted(&self, current: &OperationSnapshot, pane_id: &str) -> bool {
        if let Some(clear) = crate::tui::profile_for(&current.agent_type).clear_input_sequence() {
            let _ = crate::pty::registry_write_input(
                &self.registry,
                &self.app,
                pane_id,
                clear,
                false,
                false,
                None,
            );
        }
        // Visible-first: the turn never started, so a headless-capable agent retries headless.
        if super::fallback::convert(current, Some("PROMPT_NOT_SUBMITTED"), &self.registry) {
            super::fallback::start_headless(
                &current.project_path,
                &current.operation_id,
                &self.registry,
                &self.app,
            );
            return true;
        }
        let error = OperationError::new(
            "PROMPT_NOT_SUBMITTED",
            "the pane stayed idle and showed no output for this turn; the prompt was probably not submitted; the input box was cleared, so retrying is safe",
            true,
        );
        self.fail_run(current, error)
    }

    /// Reads the visible rows and scrollback again and tries to find the turn's reply in them.
    fn recapture(&self, pane_id: &str, agent_type: &str) -> Option<String> {
        let visible = read_tui_screen(&self.registry, pane_id).unwrap_or_default();
        let full = crate::pty::registry_read_full_text(self.registry.as_ref(), pane_id)
            .unwrap_or_default();
        completion::recapture_reply(&self.watch, &visible, &full, agent_type)
    }

    /// The wall-clock deadline passed: stop the worker and fail the run with TIMEOUT (or
    /// WORKER_STOP_FAILED when the worker could not be stopped).
    fn enforce_deadline(&self, current: &OperationSnapshot) {
        let mut error = OperationError::new(
            "TIMEOUT",
            "agent exceeded its wall-clock deadline and was terminated",
            true,
        );
        error.context = serde_json::json!({"timeout_ms": current.timeout_ms});
        if let Err(stop_error) = crate::bridge::stop_operation_worker_with_kill(
            &self.registry,
            &self.app,
            current,
            false,
        ) {
            let error = OperationError::new("WORKER_STOP_FAILED", stop_error.message, true);
            if let Ok(failed) = operations::mark_operation_state(
                &current.project_path,
                &current.operation_id,
                OperationStatus::Failed,
                StateSource::Native,
                Some("stop_failed".into()),
                None,
                Some(error),
            ) {
                self.publish(&failed);
            }
            return;
        }
        if let Ok(failed) = operations::mark_operation_state(
            &current.project_path,
            &current.operation_id,
            OperationStatus::Failed,
            StateSource::Native,
            Some("timeout".into()),
            None,
            Some(error),
        ) {
            self.publish(&failed);
        }
    }
}

/// The supervisor's clock: reads the run's snapshot every `poll`, ends when it is terminal or a
/// tick finishes it, and returns the run's snapshot once its wall-clock `timeout_ms` has passed
/// (the caller then stops the worker and fails the run).
///
/// Whatever a tick answers (including `Redelivered`, which an undeliverable prompt answers every
/// tick), the deadline is checked: a run must never stay `running` past its timeout.
fn supervise_loop(
    snapshot: &OperationSnapshot,
    poll: Duration,
    tick: &mut dyn FnMut(&OperationSnapshot) -> Step,
) -> Option<OperationSnapshot> {
    let start = snapshot.started_at_ms.unwrap_or(snapshot.created_at_ms);
    let deadline_ms = snapshot.timeout_ms;
    // A snapshot read can fail transiently (file briefly locked mid-write). Giving up on the
    // first failure would stop deadline enforcement for good and leave the turn `running`.
    let mut read_failures = 0u32;
    loop {
        let current = match operations::get_operation(&snapshot.project_path, &snapshot.operation_id) {
            Ok(current) => {
                read_failures = 0;
                current
            }
            Err(_) => {
                read_failures += 1;
                if read_failures > SUPERVISOR_MAX_READ_FAILURES {
                    return None;
                }
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        if current.status.terminal() {
            return None;
        }
        if matches!(tick(&current), Step::Finished) {
            return None;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if now.saturating_sub(start) >= deadline_ms {
            return Some(current);
        }
        // Native completion is only noticed by this poll (the wait wakes on the terminal
        // status it writes), so the interval is added to every run's latency.
        thread::sleep(poll);
    }
}

#[cfg(test)]
mod loop_tests;

fn verifies_delivery(agent_type: &str) -> bool {
    crate::tui::profile_for(agent_type).verifies_prompt_delivery()
}
