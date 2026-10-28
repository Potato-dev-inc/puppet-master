// Night Shift on the phone (SPEC n32 3.10.4): a read-only card under the Agents title, inside its scrolling list
// (AgentsOverview takes it as the `night` slot). It shows the summary line of the current (or last) night and
// the first three task rows, then "and N more" to show them all, from `GET /night`, which is the redacted view: no project
// names, no paths, no pane ids, no agent sentences. Stop shows only with the `control` cap, while the night runs,
// and asks once more. Keep and Throw away are on the computer, never here.
import { useCallback, useEffect, useState } from 'react';
import type { NightTaskView, NightView } from '../../lib/night';
import { ngrokRequestHeaders } from '../../lib/bridge-ngrok';
import { localSecretHeaders } from '../../lib/bridge-local-secret';
import { mergeBridgeHeaders } from '../../lib/mobile-pairing-auth';

/** A row shows the first characters of the task title (the view already cut it to 80). */
export const ROW_TITLE_CHARS = 60;
/** The task rows shown at first; the rest sit behind an "and N more" button. */
export const TASK_ROWS_FIRST = 3;
/** The read repeats this often while the card is up (a night starts or ends on the PC at any time). */
const REFRESH_MS = 10_000;
const STOP_FAILED = 'Could not stop Night Shift. Try again.';
const STOP_ASK = 'Stop Night Shift now? Tasks still running are cut off. Their copies are kept.';
const COMPUTER_ONLY = 'Keep and Throw away are on your computer.';

/** What the card reads and stops. `read` resolves null when no night has run yet (the PC answers 404). */
export interface NightPhoneApi {
  read: () => Promise<NightView | null>;
  stop: () => Promise<NightView>;
}

/** The phone's two Night Shift calls, with the same headers as the bridge client. */
export function bridgeNightApi(baseUrl: string): NightPhoneApi {
  const headers = (): Record<string, string> =>
    mergeBridgeHeaders({ ...ngrokRequestHeaders(baseUrl), ...localSecretHeaders(baseUrl) });
  return {
    read: async () => {
      const res = await fetch(`${baseUrl}/night`, { headers: headers() });
      if (res.status === 404) return null;
      if (!res.ok) throw new Error(`bridge GET /night -> ${res.status}`);
      return (await res.json()) as NightView;
    },
    stop: async () => {
      const res = await fetch(`${baseUrl}/night/stop`, {
        method: 'POST',
        headers: { ...headers(), 'Content-Type': 'application/json' },
        body: '{}',
      });
      if (!res.ok) throw new Error(`bridge POST /night/stop -> ${res.status}`);
      return (await res.json()) as NightView;
    },
  };
}

/** The first `max` characters (code points) of `text`; when cut, the last one is an ellipsis. */
export function firstChars(text: string, max: number): string {
  const chars = Array.from(text);
  return chars.length <= max ? text : `${chars.slice(0, max - 1).join('')}…`;
}

/** One mark per task: a copy to look at, a finished one, a copy that did not run, one still going, one waiting. */
function markOf(task: NightTaskView): string {
  if (task.class === 'needs_look') return '!';
  if (task.class === 'ready' || task.class === 'no_checks') return '✓';
  if (task.class === 'no_changes' || task.class === 'didnt_run') return '–';
  return task.state === 'working' ? '›' : '·';
}

export function NightSummary({
  api,
  canControl,
  refreshKey = 0,
}: {
  api: NightPhoneApi | null;
  /** This phone may stop a night (`control`). Without it the card only reads. */
  canControl: boolean;
  /** Bumped when the stream resynced: the card reads again at once. */
  refreshKey?: number;
}) {
  const [view, setView] = useState<NightView | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    if (!api) return;
    let live = true;
    const load = () => {
      api
        .read()
        .then((next) => {
          if (!live) return;
          setView(next);
          // The PC stops at its next tick: until the run says so, the card keeps "Stopping".
          if (next?.state !== 'running') setStopping(false);
        })
        // The card is optional: a refused or failed read keeps what is shown.
        .catch(() => undefined);
    };
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [api, refreshKey]);

  const stop = useCallback(() => {
    if (!api) return;
    setBusy(true);
    setError(null);
    api
      .stop()
      .then((next) => {
        setView(next);
        setStopping(true);
      })
      .catch(() => setError(STOP_FAILED))
      .finally(() => {
        setBusy(false);
        setConfirming(false);
      });
  }, [api]);

  if (!view) return null;
  const canStop = canControl && view.state === 'running' && !stopping;
  const shown = showAll ? view.tasks : view.tasks.slice(0, TASK_ROWS_FIRST);
  const hidden = view.tasks.length - shown.length;

  return (
    <div className="ph-night" data-night-state={view.state}>
      <section aria-label="Night Shift" className="ph-cards">
        <div className="ph-card">
          <div className="ph-card-cap">
            <span>Night Shift</span>
          </div>
          <p className="ph-card-q">{view.line}</p>
          {error && <p className="ph-appr-status">{error}</p>}
          {stopping && <p className="ph-appr-status">Stopping…</p>}
          {canStop && !confirming && (
            <div className="ph-appr-actions">
              <button type="button" className="ph-btn" disabled={busy} onClick={() => setConfirming(true)}>
                Stop Night Shift
              </button>
            </div>
          )}
          {canStop && confirming && (
            <div className="ph-appr-actions">
              <p className="ph-appr-status">{STOP_ASK}</p>
              <button type="button" className="ph-btn ph-btn--primary" disabled={busy} onClick={stop}>
                Stop now
              </button>
              <button type="button" className="ph-btn ph-btn--plain" disabled={busy} onClick={() => setConfirming(false)}>
                Keep running
              </button>
            </div>
          )}
        </div>
      </section>
      {view.tasks.length > 0 && (
        <section aria-label="Night Shift tasks">
          <ul className="ph-group">
            {shown.map((task) => (
              <li key={task.id} data-task-state={task.state}>
                <div className="ph-row">
                  <span className="ph-glyph" aria-hidden="true">
                    {markOf(task)}
                  </span>
                  <span className="ph-txt">
                    <span className="ph-t">{firstChars(task.title, ROW_TITLE_CHARS)}</span>
                    <span className="ph-s">{task.headline ?? task.stateText}</span>
                  </span>
                </div>
              </li>
            ))}
          </ul>
          {(hidden > 0 || (showAll && view.tasks.length > TASK_ROWS_FIRST)) && (
            <div className="ph-appr-actions">
              {hidden > 0 && (
                <button type="button" className="ph-btn ph-btn--plain" aria-expanded={false} onClick={() => setShowAll(true)}>
                  {`and ${hidden} more`}
                </button>
              )}
              {showAll && view.tasks.length > TASK_ROWS_FIRST && (
                <button type="button" className="ph-btn ph-btn--plain" aria-expanded={true} onClick={() => setShowAll(false)}>
                  Show fewer
                </button>
              )}
            </div>
          )}
        </section>
      )}
      <p className="ph-foot">{COMPUTER_ONLY}</p>
    </div>
  );
}
