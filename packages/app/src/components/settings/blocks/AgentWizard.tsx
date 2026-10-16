// Settings > Agents > Your own agents > Add with wizard (docs/specs/agent-wizard/SPEC.md section 9).
// A dialog with four steps: which command, who writes the profile, the automatic phase (read the help,
// draft, check, fix, live test) and the review. The wizard only writes an UNAPPROVED definition; the
// person approves it in the normal approval dialog, which the parent opens from `onApprove`.
// Everything the program printed is shown as React text, never as HTML.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  chooseDefaultDrafter,
  commandLine,
  factsFrom,
  MAX_ROUNDS,
  openRouterSupport,
  orderDrafters,
  promptBudget,
  provenanceOf,
  runDraftLoop,
  splitError,
  tauriWizardApi,
  type Drafter,
  type FinishResult,
  type LiveResult,
  type LocalDrafter,
  type LoopResult,
  type OpenRouterSupport,
  type PlannedRun,
  type ProbeInfo,
  type ProgressEvent,
  type StartInfo,
  type StepId,
  type VerifyResult,
  type WizardApi,
} from '../../../lib/agentWizard';
import { buildPrompt, REMOTE_PROMPT_BUDGET } from '../../../lib/agentWizardPrompt';

const BTN =
  'rounded-lg border border-pm-border px-3 py-1.5 text-xs font-medium hover:bg-pm-border/40 disabled:opacity-50';
const INPUT = 'min-w-0 flex-1 rounded-lg border border-pm-border bg-transparent px-2 py-1 text-xs';

type Phase = 'command' | 'finding' | 'drafter' | 'running' | 'review' | 'stopped';

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
  draft: 'Draft',
  check: 'Check',
  fix: 'Fix',
  live: 'Live test',
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
}: {
  api?: WizardApi;
  openRouter?: OpenRouterSupport;
  onClose: () => void;
  /** The definition was written (unapproved). The parent opens the approval dialog for it. */
  onApprove: (done: FinishResult) => void;
}) {
  const [phase, setPhase] = useState<Phase>('command');
  const [command, setCommand] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<Found | null>(null);
  const [locals, setLocals] = useState<LocalDrafter[]>([]);
  const [models, setModels] = useState<Array<{ id: string; label: string }>>([]);
  const [hasKey, setHasKey] = useState(false);
  const [choice, setChoice] = useState<Choice | null>(null);
  const [optIn, setOptIn] = useState(false);
  const [events, setEvents] = useState<ProgressEvent[]>([]);
  const [confirming, setConfirming] = useState<PlannedRun | null>(null);
  const [result, setResult] = useState<Extract<LoopResult, { status: 'tested' }> | null>(null);
  const [stopped, setStopped] = useState<{ title: string; text: string } | null>(null);
  const [showToml, setShowToml] = useState(false);
  const [busy, setBusy] = useState(false);

  const sessionRef = useRef<string | null>(null);
  const finishedRef = useRef(false);
  const abortRef = useRef<AbortController | null>(null);
  const confirmRef = useRef<((go: boolean) => void) | null>(null);

  // A wizard that is closed any other way than finishing leaves nothing behind.
  useEffect(
    () => () => {
      abortRef.current?.abort();
      confirmRef.current?.(false);
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
    () => (found ? buildPrompt(factsFrom(found.start, found.probe), { maxChars: REMOTE_PROMPT_BUDGET }) : null),
    [found],
  );

  const discardSession = useCallback(async () => {
    const id = sessionRef.current;
    sessionRef.current = null;
    if (id) await Promise.resolve(api.cancel(id)).catch(() => undefined);
  }, [api]);

  const findIt = useCallback(async () => {
    const name = command.trim();
    if (!name) return;
    setError(null);
    setPhase('finding');
    try {
      await discardSession();
      const start = await api.start(name);
      sessionRef.current = start.session;
      const probe = await api.probe(start.session);
      const list = orderDrafters(await api.drafters().catch(() => []));
      const orModels = await openRouter.models().catch(() => []);
      const key = await openRouter.hasKey().catch(() => false);
      setLocals(list);
      setModels(orModels);
      setHasKey(key);
      const first = chooseDefaultDrafter(list);
      setChoice(first ? { kind: 'local_agent', id: first.id } : orModels[0] ? { kind: 'openrouter', id: orModels[0].id } : null);
      setOptIn(false);
      setFound({ start, probe });
      setEvents([{ step: 'help', status: 'done', round: 0 }]);
      setPhase('drafter');
    } catch (e) {
      await discardSession();
      setError(splitError(e).message);
      setPhase('command');
    }
  }, [api, command, discardSession, openRouter]);

  const startDrafting = useCallback(async () => {
    if (!found || !drafter) return;
    const session = found.start.session;
    const abort = new AbortController();
    abortRef.current = abort;
    setError(null);
    setStopped(null);
    setResult(null);
    setShowToml(false);
    setEvents([{ step: 'help', status: 'done', round: 0 }]);
    setPhase('running');
    const ask = (prompt: string) =>
      drafter.kind === 'openrouter' ? openRouter.ask(drafter.model, prompt, abort.signal) : api.ask(session, drafter.id, prompt);
    try {
      const outcome = await runDraftLoop({
        api,
        session,
        facts: factsFrom(found.start, found.probe),
        ask,
        maxChars: promptBudget(drafter),
        signal: abort.signal,
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
      });
      if (abort.signal.aborted) return;
      if (outcome.status === 'tested') {
        setResult(outcome);
        setPhase('review');
      } else if (outcome.status === 'declined') {
        setStopped({ title: 'Stopped before the live test.', text: 'The draft passed the checks, but it was not tested on the real program, so it cannot be saved.' });
        setPhase('stopped');
      } else if (outcome.status === 'gave_up') {
        setStopped({ title: `No working profile after ${MAX_ROUNDS} tries.`, text: outcome.lastText });
        setPhase('stopped');
      }
    } catch (e) {
      if (abort.signal.aborted) return;
      setStopped({ title: 'The writer could not answer.', text: splitError(e).message });
      setPhase('stopped');
    }
  }, [api, drafter, found, openRouter]);

  const cancelAll = useCallback(async () => {
    abortRef.current?.abort();
    confirmRef.current?.(false);
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
          Type the command you use to start the agent. Puppet Master reads its help, writes a profile, tests it on the
          real program and hands it to you to approve. Tests run in an isolated temporary folder, never in your project.
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
            {phase === 'finding' && (
              <p role="status" className="mt-2 text-xs text-pm-muted">
                Finding the program and reading its help...
              </p>
            )}
          </div>
        )}

        {found && phase !== 'command' && phase !== 'finding' && <FoundSummary found={found} />}

        {phase === 'drafter' && found && (
          <div className="mt-4">
            <fieldset>
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
                  Secrets were removed from the program&apos;s output. Later tries send the same text plus the checker&apos;s
                  notes and what the program really printed in the live test.
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

        {(phase === 'running' || phase === 'stopped') && (
          <ProgressList events={events} phase={phase} />
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

        {phase === 'running' && confirming === null && (
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
              <button type="button" className={BTN} onClick={() => setPhase('drafter')}>
                Try again
              </button>
            </div>
          </div>
        )}

        {phase === 'review' && result && found && (
          <Review
            found={found}
            result={result}
            busy={busy}
            showToml={showToml}
            onToggleToml={() => setShowToml((v) => !v)}
            onApprove={() => void approve()}
            onRetry={() => setPhase('drafter')}
            onDiscard={() => void cancelAll()}
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

function ProgressList({ events, phase }: { events: ProgressEvent[]; phase: Phase }) {
  const fix = latest(events, 'fix');
  const rows: Array<{ id: StepId; label: string; event: ProgressEvent | undefined; hide?: boolean }> = [
    { id: 'help', label: STEP_LABEL.help, event: latest(events, 'help') },
    { id: 'draft', label: STEP_LABEL.draft, event: latest(events, 'draft') },
    { id: 'check', label: STEP_LABEL.check, event: latest(events, 'check') },
    { id: 'fix', label: fix ? `Fix (try ${fix.round} of ${MAX_ROUNDS})` : STEP_LABEL.fix, event: fix, hide: !fix },
    { id: 'live', label: STEP_LABEL.live, event: latest(events, 'live') },
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
  onToggleToml,
  onApprove,
  onRetry,
  onDiscard,
}: {
  found: Found;
  result: Extract<LoopResult, { status: 'tested' }>;
  busy: boolean;
  showToml: boolean;
  onToggleToml: () => void;
  onApprove: () => void;
  onRetry: () => void;
  onDiscard: () => void;
}) {
  const verify: VerifyResult = result.verify;
  const live: LiveResult = result.live;
  const line = commandLine(found.start.command, verify.rendered?.headless_argv);
  const passed = live.stages.filter((s) => s.status === 'passed').length;
  return (
    <div className="mt-4" data-testid="wizard-review">
      <p className="text-sm font-semibold">Ready for your approval</p>
      <div className="mt-2">
        <div className="text-xs font-medium text-pm-muted">The command Puppet Master will run</div>
        <pre className="mt-1 overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 text-xs">{line}</pre>
      </div>
      <div className="mt-3">
        <div className="text-xs font-medium text-pm-muted">What was tested</div>
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
        <p className="mt-2 text-xs text-pm-muted">
          {passed} live check{passed === 1 ? '' : 's'} passed
          {live.tested_version ? ` on version ${live.tested_version}` : ''}. {live.fixtures.length} saved sample
          {live.fixtures.length === 1 ? '' : 's'} will be kept next to the definition. Written after {result.rounds}{' '}
          {result.rounds === 1 ? 'try' : 'tries'}.
        </p>
      </div>
      {verify.warnings.length > 0 && (
        <ul className="mt-3 list-disc space-y-1 pl-5 text-xs text-pm-muted">
          {verify.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
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
