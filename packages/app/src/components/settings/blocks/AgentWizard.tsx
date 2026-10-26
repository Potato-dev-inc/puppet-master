// Settings > Agents > Your own agents > Add with wizard (docs/specs/agent-wizard/SPEC.md sections 9 and 11.11).
// A dialog: which command, then the guided capture (the program runs in a pane and the person answers its
// questions), who writes the profile, the automatic phase (draft, check, fix, visible test) and the review.
// When the help shows a one-task mode it is offered as an optional extra after the visible part is done.
// The wizard only writes an UNAPPROVED definition; the person approves it in the normal approval dialog,
// which the parent opens from `onApprove`. Everything the program printed or drew is shown as React text,
// never as HTML.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  agentErrorMessage,
  chooseDefaultDrafter,
  factsFrom,
  MAX_ROUNDS,
  offersOneTaskMode,
  openRouterSupport,
  orderDrafters,
  promptBudget,
  provenanceOf,
  retryLiveTest,
  reviewCommandLine,
  runDraftLoop,
  splitError,
  tauriWizardApi,
  type CaptureStatus,
  type Drafter,
  type FinishResult,
  type LocalDrafter,
  type LoopResult,
  type OpenRouterSupport,
  type PlannedRun,
  type PlannedVisible,
  type ProbeInfo,
  type ProgressEvent,
  type RetryResult,
  type StartInfo,
  type StepId,
  type VerifyResult,
  type VisibleTestStatus,
  type WizardApi,
} from '../../../lib/agentWizard';
import {
  loadFinishWithoutMe,
  NO_AUTO_DRAFTER_MESSAGE,
  pickAutoDrafter,
  runAutonomous,
  saveFinishWithoutMe,
  type AutonomousOutcome,
  type AutoTimings,
} from '../../../lib/agentWizardAuto';
import { buildPrompt, REMOTE_PROMPT_BUDGET, type CaptureFacts } from '../../../lib/agentWizardPrompt';
import { AgentWizardCapture, AnsweredList, agentName, BTN, INPUT } from './AgentWizardCapture';

const AUTO_ANSWER_KEY = 'pm.agentWizard.answerSetupQuestions';

/** The person's choice, kept between runs. On unless they turned it off. */
function loadAutoAnswer(): boolean {
  try {
    return globalThis.localStorage?.getItem(AUTO_ANSWER_KEY) !== 'off';
  } catch {
    return true;
  }
}

function saveAutoAnswer(on: boolean) {
  try {
    globalThis.localStorage?.setItem(AUTO_ANSWER_KEY, on ? 'on' : 'off');
  } catch {
    // a browser without storage keeps the choice for this window only
  }
}

type Phase = 'command' | 'finding' | 'capture' | 'drafter' | 'running' | 'auto' | 'review' | 'stopped';

type Tested = Extract<LoopResult, { status: 'tested' }>;

interface Found {
  start: StartInfo;
  probe: ProbeInfo;
}

interface Choice {
  kind: 'local_agent' | 'openrouter';
  /** Local agent id, or the OpenRouter model id. */
  id: string;
}

const STEP_LABEL: Record<StepId, string> = {
  help: 'Read the help',
  capture: 'Watch it work',
  draft: 'Draft',
  check: 'Check',
  fix: 'Fix',
  visible: 'Visible test',
  live: 'One-task test',
};

/** The latest event of a step, for the progress list. */
function latest(events: ProgressEvent[], step: StepId): ProgressEvent | undefined {
  for (let i = events.length - 1; i >= 0; i--) if (events[i].step === step) return events[i];
  return undefined;
}

function statusWord(e: ProgressEvent | undefined, waiting = 'Waiting'): string {
  if (!e) return waiting;
  return e.status === 'running' ? 'Working...' : e.status === 'done' ? 'Done' : 'Needs another try';
}

export function AgentWizard({
  api = tauriWizardApi,
  openRouter = openRouterSupport,
  onClose,
  onApprove,
  autoTimings,
}: {
  api?: WizardApi;
  openRouter?: OpenRouterSupport;
  /** Shorter waits for "Finish without me" (tests). */
  autoTimings?: Partial<AutoTimings>;
  onClose: () => void;
  /** The definition was written (unapproved). The parent opens the approval dialog for it. */
  onApprove: (done: FinishResult) => void;
}) {
  const [phase, setPhase] = useState<Phase>('command');
  const [command, setCommand] = useState('');
  // "Answer setup questions for me": the wizard may answer a trust or one-time approval question in the panes it
  // starts itself, in its own isolated folder. On by default; the choice is kept.
  const [autoAnswer, setAutoAnswer] = useState<boolean>(() => loadAutoAnswer());
  // "Finish without me" (SPEC 11.19): the wizard does everything itself, approval included. On by default; the choice is kept.
  const [finishWithoutMe, setFinishWithoutMe] = useState<boolean>(() => loadFinishWithoutMe());
  // Each automatic decision, in plain short English, and the capture pane as the run goes (for the preview).
  const [decisions, setDecisions] = useState<string[]>([]);
  const [autoCapture, setAutoCapture] = useState<CaptureStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<Found | null>(null);
  const [captures, setCaptures] = useState<CaptureFacts | null>(null);
  const [locals, setLocals] = useState<LocalDrafter[]>([]);
  const [models, setModels] = useState<Array<{ id: string; label: string }>>([]);
  const [hasKey, setHasKey] = useState(false);
  const [choice, setChoice] = useState<Choice | null>(null);
  const [optIn, setOptIn] = useState(false);
  const [events, setEvents] = useState<ProgressEvent[]>([]);
  const [confirming, setConfirming] = useState<PlannedRun | null>(null);
  const [confirmingVisible, setConfirmingVisible] = useState<PlannedVisible | null>(null);
  const [result, setResult] = useState<Tested | null>(null);
  const [stopped, setStopped] = useState<{
    title: string;
    text: string;
    agentError?: boolean;
    keepable?: boolean;
    auto?: boolean;
    /** The agent's pane is still open: "Show the pane" opens it. */
    paneOpen?: boolean;
    /** Retry goes on from that pane instead of starting a new one. */
    resumable?: boolean;
  } | null>(null);
  // The draft that passed the checks, kept when the program's own error stops the test: Retry runs only the tests.
  const [held, setHeld] = useState<{ toml: string; expects: Record<string, string>; verify: VerifyResult; rounds: number } | null>(null);
  // The finished visible profile while the optional one-task step runs (so a failure there loses nothing).
  const [visibleOnly, setVisibleOnly] = useState<Tested | null>(null);
  const [showToml, setShowToml] = useState(false);
  const [busy, setBusy] = useState(false);

  const sessionRef = useRef<string | null>(null);
  const finishedRef = useRef(false);
  const abortRef = useRef<AbortController | null>(null);
  const autoRunRef = useRef<{ found: Found; drafter: Drafter } | null>(null);
  const confirmRef = useRef<((go: boolean) => void) | null>(null);
  const confirmVisibleRef = useRef<((go: boolean) => void) | null>(null);
  // The pane the person confirmed for the visible test: the optional one-task step does not ask about it again.
  const confirmedPaneRef = useRef<string | null>(null);

  // A wizard that is closed any other way than finishing leaves nothing behind.
  useEffect(
    () => () => {
      abortRef.current?.abort();
      confirmRef.current?.(false);
      confirmVisibleRef.current?.(false);
      const id = sessionRef.current;
      if (id && !finishedRef.current) void Promise.resolve(api.cancel(id)).catch(() => undefined);
    },
    [api],
  );

  const drafter: Drafter | null = useMemo(() => {
    if (!choice) return null;
    if (choice.kind === 'openrouter') {
      const model = models.find((m) => m.id === choice.id);
      return { kind: 'openrouter', model: choice.id, label: model?.label ?? choice.id };
    }
    const local = locals.find((d) => d.id === choice.id);
    return local ? { kind: 'local_agent', id: local.id, label: local.label } : null;
  }, [choice, locals, models]);

  const previewPrompt = useMemo(
    () => (found ? buildPrompt(factsFrom(found.start, found.probe, captures), { maxChars: REMOTE_PROMPT_BUDGET }) : null),
    [found, captures],
  );

  const oneTaskOffered = useMemo(() => (found ? offersOneTaskMode(found.probe) : false), [found]);

  const discardSession = useCallback(async () => {
    const id = sessionRef.current;
    sessionRef.current = null;
    if (id) await Promise.resolve(api.cancel(id)).catch(() => undefined);
  }, [api]);

/** "Finish without me": capture, draft, check, fix, test, finish and approve with no click. */
  const startAutonomous = useCallback(
    async (target: Found, picked: Drafter, resume = false) => {
      const abort = new AbortController();
      abortRef.current = abort;
      autoRunRef.current = { found: target, drafter: picked };
      setError(null);
      setStopped(null);
      setAutoCapture(null);
      setResult(null);
      if (!resume) {
        setDecisions([]);
        setEvents([{ step: 'help', status: 'done', round: 0 }]);
      }
      setPhase('auto');
      const session = target.start.session;
      let outcome: AutonomousOutcome;
      try {
        outcome = await runAutonomous({
          api,
          start: target.start,
          probe: target.probe,
          drafter: picked,
          ask: (prompt) => (picked.kind === 'local_agent' ? api.ask(session, picked.id, prompt) : Promise.reject(new Error('Only a local writer is used here.'))),
          onEvent: (e) => setEvents((list) => [...list, e]),
          onDecision: (text) => setDecisions((list) => [...list, text]),
          onCaptureStatus: setAutoCapture,
          signal: abort.signal,
          timings: autoTimings,
          resume,
        });
      } catch (e) {
        if (abort.signal.aborted) return;
        setStopped({ title: 'The wizard could not go on.', text: splitError(e).message, auto: true });
        setPhase('stopped');
        return;
      }
      if (abort.signal.aborted || outcome.status === 'cancelled') return;
      if (outcome.status === 'finished') {
        finishedRef.current = true;
        sessionRef.current = null;
        onApprove(outcome.finish);
        return;
      }
      if (outcome.status === 'sign_in') {
        setStopped({ title: outcome.message, text: 'The wizard stopped and typed nothing more. Sign in to the agent in its pane (use Show the pane) or finish its setup, then press Retry.', auto: true, paneOpen: true });
      } else {
        setStopped({ title: outcome.title, text: outcome.text, agentError: outcome.agentError, auto: true, paneOpen: outcome.paneOpen, resumable: outcome.resumable });
      }
      setPhase('stopped');
    },
    [api, autoTimings, onApprove],
  );

  // Retry of a "Finish without me" run. After a question the person answers in the agent's own pane (the pane was kept open)
  // the capture goes on from there; when that pane is gone or the agent quit, or after any other stop, the capture pane is
  // closed and the whole run starts again in a fresh pane on the same session.
  const retryAuto = useCallback(async () => {
    const again = autoRunRef.current;
    if (!again) return;
    const id = again.found.start.session;
    if (stopped?.resumable) {
      const live = await Promise.resolve(api.captureStatus(id)).catch(() => null);
      if (live && live.state === 'running' && live.alive !== false) {
        void startAutonomous(again.found, again.drafter, true);
        return;
      }
    }
    await Promise.resolve(api.captureDiscard(id)).catch(() => undefined);
    void startAutonomous(again.found, again.drafter);
  }, [api, startAutonomous, stopped]);

  const findIt = useCallback(async () => {
    const name = command.trim();
    if (!name) return;
    setError(null);
    setPhase('finding');
    try {
      await discardSession();
      // With the switch off the call is exactly today's (two arguments).
      const start = finishWithoutMe ? await api.start(name, autoAnswer, true) : await api.start(name, autoAnswer);
      sessionRef.current = start.session;
      const probe = await api.probe(start.session);
      const list = orderDrafters(await api.drafters().catch(() => []));
      if (finishWithoutMe) {
        // The writer is picked for the person: Claude Code, else Codex, else the run stops with a plain message.
        const pick = pickAutoDrafter(list);
        if (!pick) {
          await discardSession();
          setError(NO_AUTO_DRAFTER_MESSAGE);
          setPhase('command');
          return;
        }
        setLocals(list);
        setChoice({ kind: 'local_agent', id: pick.id });
        setCaptures(null);
        setFound({ start, probe });
        void startAutonomous({ start, probe }, { kind: 'local_agent', id: pick.id, label: pick.label });
        return;
      }
      const orModels = await openRouter.models().catch(() => []);
      const key = await openRouter.hasKey().catch(() => false);
      setLocals(list);
      setModels(orModels);
      setHasKey(key);
      const first = chooseDefaultDrafter(list);
      setChoice(first ? { kind: 'local_agent', id: first.id } : orModels[0] ? { kind: 'openrouter', id: orModels[0].id } : null);
      setOptIn(false);
      setCaptures(null);
      setFound({ start, probe });
      setEvents([{ step: 'help', status: 'done', round: 0 }]);
      setPhase('capture');
    } catch (e) {
      await discardSession();
      setError(splitError(e).message);
      setPhase('command');
    }
  }, [api, autoAnswer, command, discardSession, finishWithoutMe, openRouter, startAutonomous]);

  /** Draft, check, fix and test. With \`oneTask\`, the visible file in \`result\` is kept and only the one-task mode is added. */
  const startDrafting = useCallback(
    async (oneTask?: Tested) => {
      if (!found || !drafter) return;
      const session = found.start.session;
      const abort = new AbortController();
      abortRef.current = abort;
      setError(null);
      setStopped(null);
      setHeld(null);
      setShowToml(false);
      if (oneTask) setVisibleOnly(oneTask);
      else setVisibleOnly(null);
      setResult(null);
      setEvents([
        { step: 'help', status: 'done', round: 0 },
        ...(captures ? [{ step: 'capture', status: 'done', round: 0 } as ProgressEvent] : []),
      ]);
      setPhase('running');
      const ask = (prompt: string) =>
        drafter.kind === 'openrouter' ? openRouter.ask(drafter.model, prompt, abort.signal) : api.ask(session, drafter.id, prompt);
      try {
        const outcome = await runDraftLoop({
          api,
          session,
          facts: factsFrom(found.start, found.probe, captures),
          ask,
          maxChars: promptBudget(drafter),
          signal: abort.signal,
          promptOptions: oneTask ? { addOneTaskMode: { toml: oneTask.toml, expects: oneTask.expects } } : undefined,
          confirmedVisible: oneTask ? confirmedPaneRef.current : null,
          onEvent: (e) => setEvents((list) => [...list, e]),
          confirmLive: (planned) =>
            new Promise<boolean>((resolve) => {
              confirmRef.current = (go) => {
                confirmRef.current = null;
                setConfirming(null);
                resolve(go);
              };
              setConfirming(planned);
            }),
          confirmVisible: (planned) =>
            new Promise<boolean>((resolve) => {
              confirmVisibleRef.current = (go) => {
                confirmVisibleRef.current = null;
                setConfirmingVisible(null);
                if (go) confirmedPaneRef.current = planned.fingerprint;
                resolve(go);
              };
              setConfirmingVisible(planned);
            }),
        });
        if (abort.signal.aborted) return;
        const keepable = Boolean(oneTask);
        if (outcome.status === 'tested') {
          setResult(outcome);
          setPhase('review');
        } else if (outcome.status === 'declined') {
          setStopped({
            title: captures ? 'Stopped before the test.' : 'Stopped before the live test.',
            text: 'The draft passed the checks, but it was not tested on the real program, so it cannot be saved.',
            keepable,
          });
          setPhase('stopped');
        } else if (outcome.status === 'agent_error') {
          setHeld({ toml: outcome.toml, expects: outcome.expects, verify: outcome.verify, rounds: outcome.rounds });
          setStopped({ ...agentErrorMessage(found.start.command, outcome.error.message), agentError: true, keepable });
          setPhase('stopped');
        } else if (outcome.status === 'gave_up') {
          setStopped({ title: `No working profile after ${MAX_ROUNDS} tries.`, text: outcome.lastText, keepable });
          setPhase('stopped');
        }
      } catch (e) {
        if (abort.signal.aborted) return;
        setStopped({ title: 'The writer could not answer.', text: splitError(e).message, keepable: Boolean(oneTask) });
        setPhase('stopped');
      }
    },
    [api, captures, drafter, found, openRouter],
  );

  // Retry after the program's own error: only the tests run again, for the draft that is kept.
  const retryLive = useCallback(async () => {
    if (!found || !held) return;
    setError(null);
    setStopped(null);
    setPhase('running');
    setEvents((list) => [
      ...list.filter((e) => e.step !== 'live' && e.step !== 'visible'),
      { step: held.verify.planned_visible ? 'visible' : 'live', status: 'running', round: held.rounds },
    ]);
    try {
      const out: RetryResult = await retryLiveTest({
        api,
        session: found.start.session,
        toml: held.toml,
        expects: held.expects,
        rounds: held.rounds,
        verify: held.verify,
        onEvent: (e) => setEvents((list) => [...list.filter((x) => x.step !== e.step), e]),
      });
      if (out.status === 'tested') {
        setResult({ status: 'tested', toml: out.toml, expects: out.expects, rounds: out.rounds, verify: out.verify, live: out.live, visible: out.visible });
        setPhase('review');
      } else if (out.status === 'agent_error') {
        setStopped({ ...agentErrorMessage(found.start.command, out.error.message), agentError: true });
        setPhase('stopped');
      } else {
        setHeld(null);
        setStopped({ title: 'The test failed again.', text: out.text });
        setPhase('stopped');
      }
    } catch (e) {
      setStopped({ title: 'The test could not run.', text: splitError(e).message });
      setPhase('stopped');
    }
  }, [api, found, held]);

  const cancelAll = useCallback(async () => {
    abortRef.current?.abort();
    confirmRef.current?.(false);
    confirmVisibleRef.current?.(false);
    await discardSession();
    onClose();
  }, [discardSession, onClose]);

  const approve = useCallback(async () => {
    if (!result || !drafter || !found) return;
    setBusy(true);
    setError(null);
    try {
      const done = await api.finish(found.start.session, provenanceOf(drafter, result.rounds));
      finishedRef.current = true;
      sessionRef.current = null;
      onApprove(done);
    } catch (e) {
      setError(splitError(e).message);
    } finally {
      setBusy(false);
    }
  }, [api, drafter, found, onApprove, result]);

  const orSelected = choice?.kind === 'openrouter';
  const canStart = Boolean(drafter) && (!orSelected || (hasKey && optIn));

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Add an agent with the wizard"
        className="max-h-full w-full max-w-xl overflow-auto rounded-xl border border-pm-border bg-pm-panel p-5"
      >
        <h3 className="text-sm font-semibold">Add an agent with the wizard</h3>
        <p className="mt-1 text-xs text-pm-muted">
          Type the command you use to start the agent. Puppet Master runs it in a pane and watches how it looks, writes a
          profile, tests it on the real program and hands it to you to approve. Everything runs in an isolated temporary
          folder, never in your project.
        </p>

        {error && (
          <p role="alert" className="mt-3 text-sm text-pm-text">
            {error}
          </p>
        )}

        {(phase === 'command' || phase === 'finding') && (
          <div className="mt-4">
            <label className="block text-xs font-medium" htmlFor="wizard-command">
              Which command starts it?
            </label>
            <div className="mt-1 flex items-center gap-2">
              <input
                id="wizard-command"
                type="text"
                value={command}
                disabled={phase === 'finding'}
                onChange={(e) => setCommand(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void findIt();
                }}
                placeholder="for example kiro-cli"
                className={INPUT}
              />
              <button type="button" className={BTN} disabled={phase === 'finding' || !command.trim()} onClick={() => void findIt()}>
                {phase === 'finding' ? 'Looking...' : 'Find it'}
              </button>
            </div>
            <label className="mt-3 flex items-start gap-2 text-xs" htmlFor="wizard-finish-without-me">
              <input
                id="wizard-finish-without-me"
                type="checkbox"
                className="mt-0.5"
                checked={finishWithoutMe}
                disabled={phase === 'finding'}
                onChange={(e) => {
                  setFinishWithoutMe(e.target.checked);
                  saveFinishWithoutMe(e.target.checked);
                }}
              />
              <span>
                Finish without me
                <span className="block text-pm-muted">
                  The wizard does everything itself: it watches the agent, writes the profile, tests it and approves it, and asks you
                  nothing on the way. It answers only the plain "trust this folder" or "allow once" questions, never "always" or "don't ask
                  again". It never signs in for you and never runs the agent with a permission bypass: if the agent needs a sign-in, it stops
                  and tells you. Everything runs in the wizard's own temporary folder, never in your project. You can still revoke or remove
                  the agent in Settings afterwards.
                </span>
              </span>
            </label>
            {!finishWithoutMe && (
              <label className="mt-3 flex items-start gap-2 text-xs" htmlFor="wizard-auto-answer">
                <input
                  id="wizard-auto-answer"
                  type="checkbox"
                  className="mt-0.5"
                  checked={autoAnswer}
                  disabled={phase === 'finding'}
                  onChange={(e) => {
                    setAutoAnswer(e.target.checked);
                    saveAutoAnswer(e.target.checked);
                  }}
                />
                <span>
                  Answer setup questions for me (only in the wizard's temp folder)
                  <span className="block text-pm-muted">
                    The agent may ask "do you trust this folder?" or "allow this once?" in the panes the wizard opens. With this on, the
                    wizard presses the plain yes. It never picks "always", "don't ask again" or "all", and never answers a sign-in, payment
                    or model question. Your own projects are never touched.
                  </span>
                </span>
              </label>
            )}
            {phase === 'finding' && (
              <p role="status" className="mt-2 text-xs text-pm-muted">
                Finding the program and reading its help...
              </p>
            )}
          </div>
        )}

        {found && phase !== 'command' && phase !== 'finding' && <FoundSummary found={found} />}

        {phase === 'capture' && found && (
          <AgentWizardCapture
            api={api}
            start={found.start}
            canSkip={oneTaskOffered}
            onDone={(c) => {
              setCaptures(c);
              setEvents([
                { step: 'help', status: 'done', round: 0 },
                { step: 'capture', status: 'done', round: 0 },
              ]);
              setPhase('drafter');
            }}
            onSkip={() => {
              setCaptures(null);
              setPhase('drafter');
            }}
            onCancel={() => void cancelAll()}
          />
        )}

        {phase === 'drafter' && found && (
          <div className="mt-4">
            {captures ? (
              <p data-testid="captured-note" className="text-xs text-pm-muted">
                Captured {captures.files.length} screen{captures.files.length === 1 ? '' : 's'} of {found.start.command}. Next, a writer
                turns them into a profile.
              </p>
            ) : (
              <p data-testid="headless-only-note" className="text-xs text-pm-muted">
                Without the visible part, only the one-task mode is written. {found.start.command} will not run in a pane until
                you add that.
              </p>
            )}
            <fieldset className="mt-3">
              <legend className="text-xs font-medium">Who writes the profile?</legend>
              <div className="mt-2 space-y-2">
                {locals.map((d) => (
                  <label key={d.id} className="flex cursor-pointer items-start gap-2 text-xs">
                    <input
                      type="radio"
                      name="wizard-drafter"
                      checked={choice?.kind === 'local_agent' && choice.id === d.id}
                      onChange={() => setChoice({ kind: 'local_agent', id: d.id })}
                    />
                    <span>
                      <span className="font-medium">{d.label}</span>
                      <span className="block text-pm-muted">Runs on this computer and uses your own plan.</span>
                    </span>
                  </label>
                ))}
                {models.length > 0 && (
                  <label className="flex cursor-pointer items-start gap-2 text-xs">
                    <input
                      type="radio"
                      name="wizard-drafter"
                      checked={orSelected}
                      onChange={() => setChoice({ kind: 'openrouter', id: choice?.kind === 'openrouter' ? choice.id : models[0].id })}
                    />
                    <span>
                      <span className="font-medium">An OpenRouter model</span>
                      <span className="block text-pm-muted">Free to try. You see exactly what is sent before it goes.</span>
                    </span>
                  </label>
                )}
                {locals.length === 0 && models.length === 0 && (
                  <p className="text-xs text-pm-muted">
                    No writer is available. Install Claude Code or Codex, or add an OpenRouter key in Settings &gt; Keys.
                  </p>
                )}
              </div>
            </fieldset>

            {orSelected && (
              <div className="mt-3 space-y-2" data-testid="openrouter-box">
                <label className="block text-xs text-pm-muted">
                  Model
                  <select
                    className="mt-1 block w-full rounded-lg border border-pm-border bg-transparent px-2 py-1 text-xs"
                    value={choice?.id}
                    onChange={(e) => setChoice({ kind: 'openrouter', id: e.target.value })}
                  >
                    {models.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.label}
                      </option>
                    ))}
                  </select>
                </label>
                {!hasKey && <p className="text-xs text-pm-text">Add your OpenRouter key in Settings &gt; Keys first.</p>}
                <p className="text-xs text-pm-muted">
                  This is exactly what will be sent to OpenRouter ({previewPrompt?.chars.toLocaleString()} characters).
                  Secrets were removed from the program&apos;s output and screens, and your test words are hidden. Later
                  tries send the same text plus the checker&apos;s notes and what the program really did in the test.
                </p>
                <pre
                  data-testid="openrouter-preview"
                  className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-pm-bg p-2 text-[11px]"
                >
                  {previewPrompt?.text}
                </pre>
                <label className="flex items-start gap-2 text-xs">
                  <input type="checkbox" checked={optIn} onChange={(e) => setOptIn(e.target.checked)} />
                  <span>I agree to send this text to OpenRouter{drafter ? ` (${drafter.label})` : ''}.</span>
                </label>
              </div>
            )}

            <div className="mt-4 flex justify-end gap-2">
              <button type="button" className={BTN} onClick={() => void cancelAll()}>
                Cancel
              </button>
              <button type="button" className={BTN} disabled={!canStart} onClick={() => void startDrafting()}>
                Start
              </button>
            </div>
          </div>
        )}

        {(phase === 'running' || phase === 'stopped' || phase === 'auto') && (
          <ProgressList events={events} phase={phase} withCapture={captures !== null || decisions.length > 0 || phase === 'auto'} />
        )}
        {(phase === 'auto' || (phase === 'stopped' && stopped?.auto)) && found && (
          <AutoPanel
            api={api}
            session={found.start.session}
            command={found.start.command}
            decisions={decisions}
            capture={autoCapture}
            running={phase === 'auto'}
            paneOpen={phase === 'auto' || stopped?.paneOpen === true}
            onCancel={() => void cancelAll()}
          />
        )}

        {phase === 'running' && found && confirmingVisible === null && latest(events, 'visible')?.status === 'running' && (
          <VisibleTestLive api={api} session={found.start.session} command={found.start.command} />
        )}

        {phase === 'running' && confirmingVisible !== null && found && (
          <div role="group" aria-label="Confirm the visible test" data-testid="visible-confirm" className="mt-3 rounded-lg border border-pm-border p-3">
            <p className="text-xs font-semibold">Ready for the visible test</p>
            <p className="mt-1 text-xs text-pm-muted">
              The wizard starts {found.start.command} in a new pane, in a new isolated folder, and checks that the profile reads it:
              the empty input box, the work, the reply (also for a task that makes it think and use a tool, and for a reply taller than
              the pane) and a permission question. It types a few small test messages, about five small messages per try, up to {MAX_ROUNDS} tries, and it uses your own {found.start.command} plan. If a question comes
              up in the pane, you answer it; the wizard answers nothing. If a fix changes the command, you are asked again.
            </p>
            <PlannedPane planned={confirmingVisible} />
            <div className="mt-3 flex justify-end gap-2">
              <button type="button" className={BTN} onClick={() => confirmVisibleRef.current?.(false)}>
                Not now
              </button>
              <button type="button" className={BTN} onClick={() => confirmVisibleRef.current?.(true)}>
                Run the visible test
              </button>
            </div>
          </div>
        )}

        {phase === 'running' && confirming !== null && found && (
          <div role="group" aria-label="Confirm the live test" data-testid="live-confirm" className="mt-3 rounded-lg border border-pm-border p-3">
            <p className="text-xs font-semibold">Ready for the live test</p>
            <p className="mt-1 text-xs text-pm-muted">
              {found.start.cost_note} It runs only these commands, on your computer, in an isolated temporary folder.
              <span className="font-mono"> &lt;test message&gt;</span> is replaced by a tiny test message. If a fix
              changes any of them, you are asked again. Up to {MAX_ROUNDS} tries in all.
            </p>
            <PlannedCommands planned={confirming} />
            <div className="mt-3 flex justify-end gap-2">
              <button type="button" className={BTN} onClick={() => confirmRef.current?.(false)}>
                Not now
              </button>
              <button type="button" className={BTN} onClick={() => confirmRef.current?.(true)}>
                Run the live test
              </button>
            </div>
          </div>
        )}

        {phase === 'running' && confirming === null && confirmingVisible === null && (
          <div className="mt-4 flex justify-end">
            <button type="button" className={BTN} onClick={() => void cancelAll()}>
              Cancel
            </button>
          </div>
        )}

        {phase === 'stopped' && stopped && (
          <div className="mt-3" data-testid="wizard-stopped">
            <p className="text-sm font-semibold">{stopped.title}</p>
            <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-pm-bg p-2 text-[11px]">{stopped.text}</pre>
            <div className="mt-3 flex justify-end gap-2">
              <button type="button" className={BTN} onClick={() => void cancelAll()}>
                Close
              </button>
              {stopped.keepable && visibleOnly && (
                <button
                  type="button"
                  className={BTN}
                  onClick={() => {
                    setResult(visibleOnly);
                    setStopped(null);
                    setPhase('review');
                  }}
                >
                  Keep it without a one-task mode
                </button>
              )}
              {stopped.auto ? (
                <button type="button" className={BTN} onClick={() => void retryAuto()}>
                  Retry
                </button>
              ) : stopped.agentError && held ? (
                <button type="button" className={BTN} onClick={() => void retryLive()}>
                  Retry
                </button>
              ) : (
                <button type="button" className={BTN} onClick={() => setPhase('drafter')}>
                  Try again
                </button>
              )}
            </div>
          </div>
        )}

        {phase === 'review' && result && found && (
          <Review
            found={found}
            result={result}
            busy={busy}
            showToml={showToml}
            oneTaskOffer={Boolean(captures) && oneTaskOffered && !result.live}
            writesWithoutAsking={Boolean(captures?.facts.writes_without_asking)}
            answeredForYou={[...(captures?.facts.answered ?? []), ...(result.visible?.answered ?? [])]}
            onToggleToml={() => setShowToml((v) => !v)}
            onApprove={() => void approve()}
            onRetry={() => setPhase('drafter')}
            onDiscard={() => void cancelAll()}
            onAddOneTask={() => void startDrafting(result)}
          />
        )}
      </div>
    </div>
  );
}

/** The exact commands the live test will run, as Rust rendered them. Shown as text, in a monospace block. */
function PlannedCommands({ planned }: { planned: PlannedRun }) {
  const quote = (a: string) => (/^<[^<>]+>$/.test(a) ? a : /[\s"]/.test(a) || a === '' ? `"${a.replace(/"/g, '\\"')}"` : a);
  const lines = planned.stages.map((s) => `${s.label}
  ${[planned.program.path, ...s.args].map(quote).join(' ')}`);
  const extra: string[] = [];
  if (planned.stdin !== null) extra.push(`The test message is sent on standard input: ${planned.stdin}`);
  for (const e of planned.env) extra.push(`Sets ${e.name}=${e.value}`);
  for (const name of planned.env_unset) extra.push(`Removes ${name}`);
  return (
    <div className="mt-2" data-testid="planned-commands">
      <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 font-mono text-[11px]">
        {[...lines, ...extra].join('\n')}
      </pre>
    </div>
  );
}

/** The exact start of the visible test's pane. */
function PlannedPane({ planned }: { planned: PlannedVisible }) {
  const quote = (a: string) => (/[\s"]/.test(a) || a === '' ? `"${a.replace(/"/g, '\\"')}"` : a);
  const extra: string[] = [];
  for (const e of planned.env) extra.push(`Sets ${e.name}=${e.value}`);
  for (const name of planned.env_unset) extra.push(`Removes ${name}`);
  return (
    <div className="mt-2" data-testid="planned-pane">
      <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 font-mono text-[11px]">
        {[[planned.program.path, ...planned.args].map(quote).join(' '), `In an isolated folder, in a pane ${planned.grid.cols} wide and ${planned.grid.rows} tall.`, ...extra].join('\n')}
      </pre>
    </div>
  );
}

/**
 * What the visible test is doing right now: the checks so far, the question in the test pane when there is one
 * (and whose it is to answer), the questions the wizard already answered, "Show the pane" and "Stop the test".
 */
function VisibleTestLive({ api, session, command }: { api: WizardApi; session: string; command: string }) {
  const [status, setStatus] = useState<VisibleTestStatus | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    const poll = () => {
      void Promise.resolve(api.testStatus(session))
        .then((s) => {
          if (alive.current) setStatus(s);
        })
        .catch(() => undefined);
    };
    poll();
    const timer = setInterval(poll, 700);
    return () => {
      alive.current = false;
      clearInterval(timer);
    };
  }, [api, session]);
  const needs = status?.needs_you ?? null;
  return (
    <div className="mt-3 text-xs" data-testid="visible-live">
      {needs && (
        <div role="status" data-testid="visible-needs-you" className="rounded-lg border border-pm-border p-3">
          <p className="font-semibold">Your turn</p>
          <p className="mt-1">{needs.message}</p>
          <div className="mt-2 flex flex-wrap gap-2">
            {api.testShow && (
              <button type="button" className={BTN} onClick={() => void Promise.resolve(api.testShow?.(session)).catch(() => undefined)}>
                Show the pane
              </button>
            )}
            {api.testAct && (
              <button type="button" className={BTN} onClick={() => void Promise.resolve(api.testAct?.(session, 'stop')).catch(() => undefined)}>
                Stop the test
              </button>
            )}
          </div>
        </div>
      )}
      <AnsweredList items={status?.answered ?? []} command={command} />
      {needs && status?.screen ? (
        <pre
          data-testid="visible-screen"
          aria-label={`What the ${agentName(command)} test pane shows now`}
          className="mt-2 max-h-48 overflow-auto whitespace-pre rounded-lg bg-pm-bg p-2 font-mono text-[10px] leading-tight"
        >
          {status.screen}
        </pre>
      ) : null}
    </div>
  );
}

function FoundSummary({ found }: { found: Found }) {
  const { start } = found;
  return (
    <div data-testid="wizard-found" className="mt-4 rounded-lg border border-pm-border p-3 text-xs">
      <p>
        Found <span className="font-semibold">{start.command}</span>
        {start.version.ok && start.version.text ? <> - {start.version.text}</> : <> - version unknown</>}
      </p>
      <p className="mt-1 break-all text-pm-muted">{start.program.display}</p>
      {start.program.kind === 'shim' && (
        <p className="mt-1 text-pm-muted">
          This is an npm launcher script. The wizard will look for the real program next to it.
        </p>
      )}
    </div>
  );
}

/**
 * "Finish without me": what the wizard decides on its own, one plain short line each, the capture steps as they go
 * and the pane's masked screen. The wizard presses nothing the person is not told about here.
 */
function AutoPanel({
  api,
  session,
  command,
  decisions,
  capture,
  running,
  paneOpen,
  onCancel,
}: {
  api: WizardApi;
  session: string;
  command: string;
  decisions: string[];
  capture: CaptureStatus | null;
  running: boolean;
  /** The agent's pane is still open ("Show the pane" has something to open). */
  paneOpen: boolean;
  onCancel: () => void;
}) {
  const steps = capture?.steps ?? [];
  return (
    <div className="mt-3 text-xs" data-testid="wizard-auto">
      <p className="font-semibold">Finishing on its own</p>
      {steps.length > 0 && (
        <ul aria-label="Capture steps" data-testid="auto-capture-steps" className="mt-2 space-y-0.5 text-pm-muted">
          {steps.map((s) => (
            <li key={s.id} data-step={s.id} data-status={s.status}>
              <span className="font-medium text-pm-text">{s.label}</span> {s.status === 'running' ? 'Working...' : s.status === 'needs_you' ? 'Looking at it' : s.status}
            </li>
          ))}
        </ul>
      )}
      <div className="mt-3" data-testid="auto-decisions">
        <p className="font-medium">What the wizard decided</p>
        {decisions.length === 0 ? (
          <p className="mt-1 text-pm-muted">Nothing yet.</p>
        ) : (
          <ul className="mt-1 space-y-0.5" aria-label="Decisions">
            {decisions.map((d, i) => (
              <li key={`${i}-${d}`} className={d.startsWith('Will run') ? 'break-all font-mono text-[11px] text-pm-muted' : undefined}>
                {d}
              </li>
            ))}
          </ul>
        )}
      </div>
      {paneOpen && (
        <div className="mt-3 flex flex-wrap gap-2">
          <button type="button" className={BTN} onClick={() => void Promise.resolve(api.captureShow(session)).catch(() => undefined)}>
            Show the pane
          </button>
        </div>
      )}
      {running && (
        <div className="mt-4 flex justify-end">
          <button type="button" className={BTN} onClick={onCancel}>
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}

function ProgressList({ events, phase, withCapture }: { events: ProgressEvent[]; phase: Phase; withCapture: boolean }) {
  const fix = latest(events, 'fix');
  const rows: Array<{ id: StepId; label: string; event: ProgressEvent | undefined; hide?: boolean }> = [
    { id: 'help', label: STEP_LABEL.help, event: latest(events, 'help') },
    { id: 'capture', label: STEP_LABEL.capture, event: latest(events, 'capture'), hide: !withCapture },
    { id: 'draft', label: STEP_LABEL.draft, event: latest(events, 'draft') },
    { id: 'check', label: STEP_LABEL.check, event: latest(events, 'check') },
    { id: 'fix', label: fix ? `Fix (try ${fix.round} of ${MAX_ROUNDS})` : STEP_LABEL.fix, event: fix, hide: !fix },
    { id: 'visible', label: STEP_LABEL.visible, event: latest(events, 'visible'), hide: !withCapture },
    { id: 'live', label: STEP_LABEL.live, event: latest(events, 'live'), hide: withCapture && !latest(events, 'live') },
  ];
  return (
    <ul aria-label="Progress" data-testid="wizard-progress" className="mt-4 space-y-1">
      {rows
        .filter((r) => !r.hide)
        .map((r) => (
          <li key={r.id} data-step={r.id} data-status={r.event?.status ?? 'waiting'} className="flex items-baseline gap-2 text-xs">
            <span className="w-44 shrink-0 font-medium">{r.label}</span>
            <span className={r.event?.status === 'failed' ? 'text-pm-text' : 'text-pm-muted'}>
              {statusWord(r.event, phase === 'stopped' ? 'Not reached' : 'Waiting')}
              {r.event?.detail && r.event.status === 'failed' ? ` - ${r.event.detail}` : ''}
            </span>
          </li>
        ))}
    </ul>
  );
}

function Review({
  found,
  result,
  busy,
  showToml,
  oneTaskOffer,
  writesWithoutAsking,
  answeredForYou,
  onToggleToml,
  onApprove,
  onRetry,
  onDiscard,
  onAddOneTask,
}: {
  found: Found;
  result: Tested;
  busy: boolean;
  showToml: boolean;
  oneTaskOffer: boolean;
  writesWithoutAsking: boolean;
  answeredForYou: Array<{ flavour: string; label: string; keys?: string; screen?: string; denied?: string }>;
  onToggleToml: () => void;
  onApprove: () => void;
  onRetry: () => void;
  onDiscard: () => void;
  onAddOneTask: () => void;
}) {
  const verify: VerifyResult = result.verify;
  const { live, visible } = result;
  const line = visible
    ? reviewCommandLine(
        verify.planned_visible?.program.display ?? found.start.program.display,
        found.start.command,
        verify.planned_visible?.args ?? verify.rendered?.tui_argv ?? [],
      )
    : reviewCommandLine(found.start.program.display, found.start.command, verify.rendered?.headless_argv);
  const livePassed = live ? live.stages.filter((s) => s.status === 'passed').length : 0;
  const liveRan = live ? live.stages.filter((s) => s.status !== 'skipped').length : 0;
  const version = visible?.version ?? live?.tested_version ?? null;
  return (
    <div className="mt-4" data-testid="wizard-review">
      <p className="text-sm font-semibold">Ready for your approval</p>
      <div className="mt-2">
        <div className="text-xs font-medium text-pm-muted">The command Puppet Master will run</div>
        <pre className="mt-1 overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 text-xs">{line}</pre>
      </div>
      <div className="mt-3">
        <div className="text-xs font-medium text-pm-muted">What was tested</div>
        {visible && (
          <div data-testid="review-visible" className="mt-1 text-xs">
            <p className="font-medium">
              Visible pane: {visible.passed} of {visible.total} checks passed
            </p>
            <AnsweredList items={answeredForYou} command={found.start.command} heading="Answered for you" />
            {writesWithoutAsking && (
              <p data-testid="review-writes-without-asking" className="mt-1 text-pm-muted">
                This agent writes files without asking first. Puppet Master saw it create a file with no question.
              </p>
            )}
            <ul className="mt-1 space-y-1">
              {visible.checks.map((c) => (
                <li key={c.name} data-check={c.name} data-status={c.status}>
                  <span className="font-medium">{c.label}</span>{' '}
                  <span className="text-pm-muted">{c.status === 'passed' ? 'passed' : c.status === 'skipped' ? `skipped - ${c.note}` : 'failed'}</span>
                </li>
              ))}
            </ul>
          </div>
        )}
        {live && !visible ? (
          <ul className="mt-1 space-y-1 text-xs">
            {live.stages.map((s) => (
              <li key={s.name} data-stage={s.name} data-status={s.status}>
                <span className="font-medium">{s.label}</span>{' '}
                <span className="text-pm-muted">
                  {s.status === 'passed' ? 'passed' : s.status === 'skipped' ? `skipped - ${s.note ?? ''}` : 'failed'}
                </span>
              </li>
            ))}
          </ul>
        ) : live ? (
          <div data-testid="review-live" className="mt-2 text-xs">
            <p className="font-medium">
              One-task mode: {livePassed} of {liveRan} checks passed
            </p>
            <ul className="mt-1 space-y-1">
              {live.stages.map((s) => (
                <li key={s.name} data-stage={s.name} data-status={s.status}>
                  <span className="font-medium">{s.label}</span>{' '}
                  <span className="text-pm-muted">
                    {s.status === 'passed' ? 'passed' : s.status === 'skipped' ? `skipped - ${s.note ?? ''}` : 'failed'}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        ) : visible ? (
          <p data-testid="review-live" className="mt-2 text-xs font-medium">
            One-task mode: not added
          </p>
        ) : null}
        {live && !visible ? (
          <p className="mt-2 text-xs text-pm-muted">
            Tested live on {found.start.command}
            {version ? ` ${version}` : ''}: {livePassed} of {liveRan} checks passed. {live.fixtures.length} saved sample
            {live.fixtures.length === 1 ? '' : 's'} will be kept next to the definition. Written after {result.rounds}{' '}
            {result.rounds === 1 ? 'try' : 'tries'}.
          </p>
        ) : (
          <p className="mt-2 text-xs text-pm-muted">
            Tested live on {found.start.command}
            {version ? ` ${version}` : ''}. The captured screens{live ? ' and the saved samples' : ''} will be kept next to the
            definition. Written after {result.rounds} {result.rounds === 1 ? 'try' : 'tries'}.
          </p>
        )}
      </div>
      {verify.warnings.length > 0 && (
        <ul className="mt-3 list-disc space-y-1 pl-5 text-xs text-pm-muted">
          {verify.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      {oneTaskOffer && (
        <div data-testid="one-task-offer" className="mt-3 rounded-lg border border-pm-border p-3 text-xs">
          <p className="font-semibold">Also add a one-task mode (optional)</p>
          <p className="mt-1 text-pm-muted">
            {found.start.command} can also run one task and print the answer. With it, Puppet Master can run it without a pane.
            This writes the extra part and tests it, and runs the visible test again on the new file (about two more small
            messages). The visible part you have now is kept if it does not work out.
          </p>
          <div className="mt-2 flex justify-end">
            <button type="button" className={BTN} disabled={busy} onClick={onAddOneTask}>
              Add the one-task mode
            </button>
          </div>
        </div>
      )}
      <p className="mt-3 text-xs text-pm-muted">
        Nothing runs until you approve it. The next screen shows the exact command again.
      </p>
      {showToml && (
        <pre
          data-testid="wizard-toml"
          className="mt-3 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-pm-bg p-2 text-[11px]"
        >
          {result.toml}
        </pre>
      )}
      <div className="mt-4 flex flex-wrap justify-end gap-2">
        <button type="button" className={BTN} onClick={onToggleToml}>
          {showToml ? 'Hide TOML' : 'Show TOML'}
        </button>
        <button type="button" className={BTN} disabled={busy} onClick={onRetry}>
          Retry
        </button>
        <button type="button" className={BTN} disabled={busy} onClick={onDiscard}>
          Discard
        </button>
        <button type="button" className={BTN} disabled={busy} onClick={onApprove}>
          Approve
        </button>
      </div>
    </div>
  );
}
