// The guided capture of the Agent Wizard (docs/specs/agent-wizard/SPEC.md sections 11.4, 11.5 and 11.11).
// The program runs in a real pane, in an isolated temporary folder. This panel shows the exact command first
// (the person confirms it), then the steps: the program starts, the person answers its questions in the pane
// (the wizard presses nothing), a test message is typed, a permission question is shown and left to the
// person. Everything the pane shows is text from the program: it is rendered as React text, never as HTML.
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  splitError,
  type CaptureAction,
  type CaptureDebug,
  type CapturePlan,
  type CaptureStatus,
  type CaptureStep,
  type CaptureStepId,
  type StartInfo,
  type WizardApi,
} from '../../../lib/agentWizard';
import type { CaptureFacts } from '../../../lib/agentWizardPrompt';

export const BTN =
  'rounded-lg border border-pm-border px-3 py-1.5 text-xs font-medium hover:bg-pm-border/40 disabled:opacity-50';
export const INPUT = 'min-w-0 flex-1 rounded-lg border border-pm-border bg-transparent px-2 py-1 text-xs';

const POLL_MS = 700;

const STATUS_WORD: Record<CaptureStep['status'], string> = {
  pending: 'Waiting',
  running: 'Working...',
  needs_you: 'Needs you',
  done: 'Done',
  skipped: 'Skipped',
  stuck: 'Stuck, kept what was captured',
  failed: 'Did not work',
};

/** "What the wizard saw": the last looks of the capture and how each was read. Loaded when the person asks. */
function SawPanel({ api, session }: { api: WizardApi; session: string }) {
  const [debug, setDebug] = useState<CaptureDebug | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const load = async () => {
    setError(null);
    try {
      setDebug((await api.captureDebug?.(session)) ?? null);
    } catch (e) {
      setError(splitError(e).message);
    }
  };
  const frames = debug?.frames ?? [];
  const last = frames[frames.length - 1];
  return (
    <div className="mt-3 text-xs" data-testid="capture-saw">
      <button type="button" className={BTN} onClick={() => void load()}>
        What the wizard saw
      </button>
      {error && <p className="mt-1 text-pm-text">{error}</p>}
      {debug === null && <p className="mt-1 text-pm-muted">Nothing was kept yet.</p>}
      {last && (
        <div className="mt-2">
          <ul aria-label="The last looks" className="space-y-0.5 text-pm-muted">
            {frames.slice(-6).map((f) => (
              <li key={f.ms} data-ms={f.ms}>
                {Math.round(f.ms / 1000)} s, {f.step}: read as {f.classifier}
                {f.reading !== f.classifier ? `, then ${f.reading}` : ''}; still for {Math.round(f.stable_ms / 1000)} s, nothing moved for{' '}
                {Math.round(f.quiet_ms / 1000)} s
              </li>
            ))}
          </ul>
          <pre
            data-testid="capture-saw-screen"
            className="mt-2 max-h-40 overflow-auto whitespace-pre rounded-lg bg-pm-bg p-2 font-mono text-[10px] leading-tight"
          >
            {last.screen}
          </pre>
        </div>
      )}
    </div>
  );
}

/** Split what the person typed into arguments (spaces separate them; double quotes keep words together). */
export function splitArgs(text: string): string[] {
  const args: string[] = [];
  const re = /"([^"]*)"|(\S+)/g;
  for (let m = re.exec(text); m; m = re.exec(text)) args.push(m[1] ?? m[2]);
  return args;
}

/** The command as one line the person can read. */
export function commandText(plan: CapturePlan): string {
  const quote = (a: string) => (/[\s"]/.test(a) || a === '' ? `"${a.replace(/"/g, '\\"')}"` : a);
  return [plan.program.path, ...plan.args].map(quote).join(' ');
}

export function AgentWizardCapture({
  api,
  start,
  canSkip,
  onDone,
  onSkip,
  onCancel,
}: {
  api: WizardApi;
  start: StartInfo;
  /** The help shows a one-task mode, so the visible part may be skipped. */
  canSkip: boolean;
  /** The capture is finished: the screens, masked, as the drafter will see them. */
  onDone: (captures: CaptureFacts) => void;
  onSkip: () => void;
  onCancel: () => void;
}) {
  const session = start.session;
  const [argsText, setArgsText] = useState('');
  const [plan, setPlan] = useState<CapturePlan | null>(null);
  const [status, setStatus] = useState<CaptureStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // The plan (the exact command) for the arguments typed so far.
  const refreshPlan = useCallback(
    async (text: string) => {
      try {
        const next = await api.capturePlan(session, splitArgs(text));
        if (alive.current) {
          setPlan(next);
          setError(null);
        }
      } catch (e) {
        if (alive.current) setError(splitError(e).message);
      }
    },
    [api, session],
  );

  useEffect(() => {
    void refreshPlan('');
  }, [refreshPlan]);

  const running = status !== null && status.state === 'running';

  // Poll while the capture runs.
  useEffect(() => {
    if (!running) return undefined;
    const timer = setInterval(() => {
      void api
        .captureStatus(session)
        .then((s) => {
          if (alive.current) setStatus(s);
        })
        .catch((e) => {
          if (alive.current) setError(splitError(e).message);
        });
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [api, running, session]);

  const begin = useCallback(async () => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      const s = await api.captureStart(session, plan.args, plan.fingerprint);
      if (alive.current) setStatus(s);
    } catch (e) {
      if (alive.current) setError(splitError(e).message);
    } finally {
      if (alive.current) setBusy(false);
    }
  }, [api, plan, session]);

  const act = useCallback(
    async (action: CaptureAction, step?: CaptureStepId) => {
      setError(null);
      try {
        const s = await api.captureAct(session, action, step);
        if (alive.current) setStatus(s);
      } catch (e) {
        if (alive.current) setError(splitError(e).message);
      }
    },
    [api, session],
  );

  const finish = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const captures = await api.captureFinish(session);
      if (!alive.current) return;
      if (!captures || captures.files.length === 0) {
        setStatus(null);
        setError('Nothing was captured. Start again, or go back.');
      } else {
        onDone(captures);
      }
    } catch (e) {
      if (alive.current) setError(splitError(e).message);
    } finally {
      if (alive.current) setBusy(false);
    }
  }, [api, onDone, session]);

  const cancel = useCallback(async () => {
    await Promise.resolve(api.captureDiscard(session)).catch(() => undefined);
    onCancel();
  }, [api, onCancel, session]);

  if (!running) {
    return (
      <div className="mt-4" data-testid="capture-plan">
        <p className="text-xs font-semibold">Watch it work once</p>
        <p className="mt-1 text-xs text-pm-muted">
          Puppet Master starts {start.command} in a pane, in an isolated temporary folder (never your project). You answer
          its questions in the pane yourself: the wizard presses nothing for you. It types two small test messages and
          writes down what the screen looks like. This uses a little of your {start.command} plan.
        </p>
        <div className="mt-3 text-xs font-medium text-pm-muted">It will start exactly this</div>
        <pre data-testid="capture-command" className="mt-1 overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-pm-bg p-2 font-mono text-[11px]">
          {plan ? commandText(plan) : 'Reading the command...'}
        </pre>
        {plan && (
          <p className="mt-1 text-xs text-pm-muted">
            In an isolated folder, in a pane {plan.grid.cols} wide and {plan.grid.rows} tall.
          </p>
        )}
        <div className="mt-3 flex items-center gap-2">
          <label className="text-xs text-pm-muted" htmlFor="capture-args">
            Extra arguments (optional)
          </label>
          <input
            id="capture-args"
            type="text"
            className={INPUT}
            value={argsText}
            placeholder="for example --model small"
            onChange={(e) => {
              setArgsText(e.target.value);
              void refreshPlan(e.target.value);
            }}
          />
        </div>
        {error && (
          <p role="alert" className="mt-3 text-sm text-pm-text">
            {error}
          </p>
        )}
        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <button type="button" className={BTN} onClick={() => void cancel()}>
            Cancel
          </button>
          {canSkip ? (
            <button type="button" className={BTN} onClick={onSkip}>
              Skip the visible part
            </button>
          ) : (
            <span className="self-center text-xs text-pm-muted">It shows no one-task mode, so the pane is the way in.</span>
          )}
          <button type="button" className={BTN} disabled={!plan || busy} onClick={() => void begin()}>
            Start
          </button>
        </div>
      </div>
    );
  }

  const steps = status?.steps ?? [];
  const current = steps.find((s) => s.id === status?.step);
  const can = new Set(status?.can ?? []);
  const needs = status?.needs_you ?? null;
  return (
    <div className="mt-4" data-testid="capture-running">
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs font-semibold">Watching {start.command} in a pane</p>
        <button type="button" className={BTN} onClick={() => void api.captureShow(session).catch(() => undefined)}>
          Show the pane
        </button>
      </div>
      <ul aria-label="Capture steps" data-testid="capture-steps" className="mt-3 space-y-2">
        {steps.map((s) => (
          <li key={s.id} data-step={s.id} data-status={s.status} className="text-xs">
            <div className="flex items-baseline gap-2">
              <span className="w-28 shrink-0 font-medium">{s.label}</span>
              <span className={s.status === 'needs_you' || s.status === 'failed' ? 'text-pm-text' : 'text-pm-muted'}>{STATUS_WORD[s.status]}</span>
            </div>
            {s.note && s.id === status?.step && s.status !== 'needs_you' && <p className="mt-0.5 pl-[7.5rem] text-pm-muted">{s.note}</p>}
          </li>
        ))}
      </ul>
      {needs && (
        <div role="status" data-testid="capture-needs-you" className="mt-3 rounded-lg border border-pm-border p-3 text-xs">
          <p className="font-semibold">Your turn</p>
          <p className="mt-1">{needs.message}</p>
        </div>
      )}
      <div className="mt-3 flex flex-wrap gap-2">
        {can.has('ready') && (
          <button type="button" className={BTN} onClick={() => void act('ready')}>
            It is ready
          </button>
        )}
        {can.has('try_exit') && (
          <button type="button" className={BTN} onClick={() => void act('try_exit')}>
            Try /exit
          </button>
        )}
        {can.has('closed') && (
          <button type="button" className={BTN} onClick={() => void act('closed')}>
            I closed it myself
          </button>
        )}
        {can.has('done') && (
          <button type="button" className={BTN} onClick={() => void act('done')}>
            It is done
          </button>
        )}
        {can.has('question') && (
          <button type="button" className={BTN} onClick={() => void act('question')}>
            It is a question
          </button>
        )}
        {can.has('retry') && current && (
          <button type="button" className={BTN} onClick={() => void act('retry')}>
            Retry
          </button>
        )}
        {can.has('stuck') && (
          <button type="button" className={BTN} onClick={() => void act('stuck')}>
            It looks stuck
          </button>
        )}
        {can.has('skip') && (
          <button type="button" className={BTN} onClick={() => void act('skip')}>
            Skip
          </button>
        )}
      </div>
      {(needs || status?.frozen) && api.captureDebug ? <SawPanel api={api} session={session} /> : null}
      {status?.screen ? (
        <pre
          data-testid="capture-screen"
          aria-label="What the pane shows now"
          className="mt-3 max-h-56 overflow-auto whitespace-pre rounded-lg bg-pm-bg p-2 font-mono text-[10px] leading-tight"
        >
          {status.screen}
        </pre>
      ) : null}
      {error && (
        <p role="alert" className="mt-3 text-sm text-pm-text">
          {error}
        </p>
      )}
      <div className="mt-4 flex flex-wrap justify-end gap-2">
        <button type="button" className={BTN} disabled={busy} onClick={() => void cancel()}>
          Cancel
        </button>
        <button type="button" className={BTN} disabled={busy} onClick={() => void finish()}>
          {status?.settled ? 'Use these screens' : 'Use what was captured so far'}
        </button>
      </div>
    </div>
  );
}
