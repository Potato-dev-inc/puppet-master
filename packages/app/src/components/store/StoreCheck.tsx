// The Check sheet of the Agent Store. It says what a check runs and what it costs, does nothing until Start is
// pressed, then shows the wizard's progress and the result. The run itself is passed in as `runCheck` (the real one
// is wired in AS-20), so this file never calls Tauri and never writes anything.
import { useEffect, useRef, useState } from 'react';
import type { ProgressEvent, StepId } from '../../lib/agentWizard';
import { DotSpinner } from '../cockpit/DotSpinner';
import { Sheet } from '../cockpit/Sheet';

/** What a finished check reports. Numbers, a version and plain sentences only: no screens, no answers. */
export type StoreCheckResult =
  | { status: 'done'; passed: number; total: number; version: string; note?: string }
  /** The agent asks for a sign-in. The person signs in in the agent's own pane, then presses Retry. */
  | { status: 'sign_in'; message: string }
  | { status: 'stopped'; message: string };

/** Runs the check for one catalog id. It must settle soon after `signal` aborts. */
export type RunStoreCheck = (
  id: string,
  signal: AbortSignal,
  onEvent: (event: ProgressEvent) => void,
) => Promise<StoreCheckResult>;

const STEP_ORDER: StepId[] = ['help', 'capture', 'draft', 'check', 'fix', 'visible', 'live'];

const STEP_LABEL: Record<StepId, string> = {
  help: 'Read the help',
  capture: 'Watch it work',
  draft: 'Draft',
  check: 'Check',
  fix: 'Fix',
  visible: 'Visible test',
  live: 'One-task test',
};

type View =
  | { phase: 'ready' }
  | { phase: 'running' }
  | { phase: 'stopping' }
  | { phase: 'result'; result: StoreCheckResult }
  | { phase: 'error'; message: string }
  | { phase: 'cancelled' };

export interface StoreCheckProps {
  id: string;
  label: string;
  /** The version found on this PC now, or null when the program is not found. */
  installedVersion: string | null;
  runCheck: RunStoreCheck;
  /** Opens the agent in its own pane, so the person can sign in there. */
  onOpenAgent: (id: string) => void;
  onClose: () => void;
}

/** The latest event of a step, for the progress list. */
function latest(events: ProgressEvent[], step: StepId): ProgressEvent | undefined {
  for (let i = events.length - 1; i >= 0; i--) if (events[i].step === step) return events[i];
  return undefined;
}

function statusWord(e: ProgressEvent): string {
  return e.status === 'running' ? 'Working...' : e.status === 'done' ? 'Done' : 'Needs another try';
}

function errorText(e: unknown): string {
  if (typeof e === 'string' && e) return e;
  if (e instanceof Error && e.message) return e.message;
  return 'The check could not finish.';
}

function ProgressRows({ events }: { events: ProgressEvent[] }) {
  const rows = STEP_ORDER.map((step) => ({ step, event: latest(events, step) })).filter((r) => r.event);
  if (rows.length === 0) return null;
  return (
    <ul aria-label="Progress" data-testid="store-check-progress" className="ck-sub">
      {rows.map(({ step, event }) => (
        <li key={step} data-step={step} data-status={event!.status}>
          {STEP_LABEL[step]}: {statusWord(event!)}
          {event!.detail && event!.status === 'failed' ? ` - ${event!.detail}` : ''}
        </li>
      ))}
    </ul>
  );
}

export function StoreCheck({ id, label, installedVersion, runCheck, onOpenAgent, onClose }: StoreCheckProps) {
  const [view, setView] = useState<View>({ phase: 'ready' });
  const [events, setEvents] = useState<ProgressEvent[]>([]);
  // Each Start gets a token. Events and results from an older run are ignored.
  const runSeq = useRef(0);
  const current = useRef<AbortController | null>(null);

  // A run still going when the sheet goes away is aborted, and its late results are dropped.
  useEffect(
    () => () => {
      runSeq.current += 1;
      current.current?.abort();
    },
    [],
  );

  const start = () => {
    const token = ++runSeq.current;
    const ctrl = new AbortController();
    current.current = ctrl;
    setEvents([]);
    setView({ phase: 'running' });
    const live = () => runSeq.current === token;
    const onEvent = (event: ProgressEvent) => {
      if (live()) setEvents((prev) => [...prev, event]);
    };
    const settle = (next: View) => {
      if (live()) setView(next);
    };
    let pending: Promise<StoreCheckResult>;
    try {
      pending = Promise.resolve(runCheck(id, ctrl.signal, onEvent));
    } catch (e) {
      pending = Promise.reject(e);
    }
    pending.then(
      (result) => settle(ctrl.signal.aborted ? { phase: 'cancelled' } : { phase: 'result', result }),
      (e: unknown) => settle(ctrl.signal.aborted ? { phase: 'cancelled' } : { phase: 'error', message: errorText(e) }),
    );
  };

  const stop = () => {
    current.current?.abort();
    setView({ phase: 'stopping' });
  };

  const busy = view.phase === 'running' || view.phase === 'stopping';
  const result = view.phase === 'result' ? view.result : null;

  return (
    <Sheet labelledBy="ck-store-check-t" onClose={busy ? undefined : onClose}>
      <h2 className="ck-section" id="ck-store-check-t">
        Check {label}
      </h2>

      {view.phase === 'ready' && (
        <>
          <p className="ck-sub">
            Puppet Master starts {label} in its own pane, in a new temporary folder kept apart from your projects. It reads
            the answers from the screen.
          </p>
          <p className="ck-sub">What will run:</p>
          <ul className="ck-sub" data-testid="store-check-runs">
            <li>
              {label}, {installedVersion ? `version ${installedVersion}` : 'the program found on this PC'}.
            </li>
            <li>A few short test messages, and the answers on the screen.</li>
            <li>Only trust and allow-once questions are answered, and only in that pane.</li>
            <li>No file is kept, and nothing is added or approved.</li>
          </ul>
          <p className="ck-sub" data-testid="store-check-cost">
            A check uses real agent turns from your own plan. It uses about 4 small messages on your own account.
          </p>
          <p className="ck-sub">A sign-in screen stops the check. Sign in in the agent's own pane, then press Retry.</p>
        </>
      )}

      {busy && (
        <p className="ck-sub">
          <DotSpinner status="running" label={`Checking ${label}`} /> Checking {label}. Puppet Master works in its own pane.
        </p>
      )}

      <ProgressRows events={events} />

      {result?.status === 'done' && (
        <>
          <p className="ck-section" data-testid="store-check-result">
            {result.passed} of {result.total} checks passed on version {result.version}.
          </p>
          {result.note && <p className="ck-sub">{result.note}</p>}
          <p className="ck-sub">Nothing was added or approved.</p>
        </>
      )}
      {result?.status === 'sign_in' && (
        <p className="ck-sub" data-testid="store-check-sign-in">
          {result.message || `Sign in to ${label} in its own pane, then press Retry.`}
        </p>
      )}
      {result?.status === 'stopped' && <p className="ck-sub">{result.message}</p>}
      {view.phase === 'cancelled' && <p className="ck-sub">Check stopped. Nothing was added or approved.</p>}
      {view.phase === 'error' && <p className="ck-sub ck-doctor-error">{view.message}</p>}

      <div className="ck-sheet-actions">
        {view.phase === 'ready' && (
          <>
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={onClose}>
              Cancel
            </button>
            <button type="button" className="ck-btn ck-btn--primary" onClick={start}>
              Start
            </button>
          </>
        )}
        {view.phase === 'running' && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={stop}>
            Stop
          </button>
        )}
        {view.phase === 'stopping' && (
          <button type="button" className="ck-btn ck-btn--secondary" disabled>
            Stopping...
          </button>
        )}
        {result?.status === 'sign_in' && (
          <>
            <button type="button" className="ck-btn ck-btn--secondary" onClick={onClose}>
              Close
            </button>
            <button type="button" className="ck-btn ck-btn--secondary" onClick={start}>
              Retry
            </button>
            <button type="button" className="ck-btn ck-btn--primary" onClick={() => onOpenAgent(id)}>
              Open agent
            </button>
          </>
        )}
        {(view.phase === 'result' && result?.status !== 'sign_in') || view.phase === 'error' || view.phase === 'cancelled' ? (
          <>
            <button type="button" className="ck-btn ck-btn--secondary" onClick={onClose}>
              Close
            </button>
            <button type="button" className="ck-btn ck-btn--primary" onClick={start}>
              {result?.status === 'done' ? 'Check again' : 'Retry'}
            </button>
          </>
        ) : null}
      </div>
    </Sheet>
  );
}
