// "What broke the tests?" from History and the palette. One sheet, three stages:
// set up (the test command, the save point to start from, the limits), running (one plain status line
// and Stop) and the result (`renderResult`, the result component once it is wired in).
// The sheet only calls `lib/what-broke-it.ts`; it never writes to a pane or to the person's folder.
import { useEffect, useState, type ReactNode } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
  onWhatBrokeItChanged,
  wbDeclare,
  wbFinding,
  wbOverview,
  wbStart,
  wbStatus,
  wbStop,
  type FindingView,
  type JobView,
  type Limits,
  type Overview,
} from '../../lib/what-broke-it';
import '../../styles/cockpit-whatbrokeit.css';
import type { Notify } from './restore-flow';
import { Sheet } from './Sheet';

/** The calls the sheet makes. The desktop window uses `whatBrokeItApi`; tests pass their own. */
export const whatBrokeItApi = { wbOverview, wbDeclare, wbStart, wbStatus, wbStop, wbFinding, onWhatBrokeItChanged };
export type WhatBrokeItApi = typeof whatBrokeItApi;

/** Timeout for a test command the person saves from here (minutes). */
const DEFAULT_TIMEOUT_MIN = 10;

/** Clamp to the backend range (`Limits`): runs 1..=30, minutes 1..=60. */
export function clampLimits(limits: Limits): Limits {
  return {
    max_runs: Math.min(30, Math.max(1, Math.round(limits.max_runs))),
    max_minutes: Math.min(60, Math.max(1, Math.round(limits.max_minutes))),
  };
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

function ResultPlain({ finding, onClose }: { finding: FindingView; onClose: () => void }) {
  return (
    <>
      <p className="ck-wb-result-title">{finding.title}</p>
      <p className="ck-sub">{finding.subtitle}</p>
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--primary" onClick={onClose}>
          Close
        </button>
      </div>
    </>
  );
}

export function WhatBrokeItSheet({
  paneId,
  notify,
  onClose,
  sharedWith = [],
  api = whatBrokeItApi,
  renderResult,
}: {
  paneId: string;
  notify: Notify;
  onClose: () => void;
  /** Labels of the other live panes working in this pane's folder. Their turns count too. */
  sharedWith?: string[];
  api?: WhatBrokeItApi;
  /** The result component for a finished search. Defaults to the plain title and subtitle. */
  renderResult?: (finding: FindingView) => ReactNode;
}) {
  const [overview, setOverview] = useState<Overview | null>(null);
  /** The declared test the person picked (its `index`); null means the first one. */
  const [picked, setPicked] = useState<number | null>(null);
  const [editing, setEditing] = useState(false);
  const [typed, setTyped] = useState('');
  /** Save point to start from; '' means "Find it for me". */
  const [goodId, setGoodId] = useState('');
  const [limits, setLimits] = useState<Limits | null>(null);
  const [editingLimits, setEditingLimits] = useState(false);
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [job, setJob] = useState<JobView | null>(null);
  const [finding, setFinding] = useState<FindingView | null>(null);

  useEffect(() => {
    let alive = true;
    api.wbOverview(paneId).then(
      (view) => {
        if (alive) setOverview(view);
      },
      (e) => {
        if (!alive) return;
        notify(errText(e));
        onClose();
      },
    );
    return () => {
      alive = false;
    };
    // Load once per open: notify/onClose identities change every render of the parent.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, paneId]);

  // While a search runs: follow its events, and read the status once so a missed event cannot hide the end.
  const jobId = job?.id ?? null;
  useEffect(() => {
    if (!jobId) return;
    let alive = true;
    let unlisten: UnlistenFn | null = null;
    const look = async () => {
      try {
        const now = await api.wbStatus(jobId);
        if (!alive) return;
        setJob(now);
        if (now.findingId) {
          const result = await api.wbFinding(now.findingId);
          if (alive) setFinding(result);
        }
      } catch (e) {
        if (alive) setError(errText(e));
      }
    };
    api.onWhatBrokeItChanged((event) => {
      if (event.jobId === jobId) void look();
    }).then(
      (stop) => {
        if (alive) unlisten = stop;
        else stop();
      },
      (e) => {
        if (alive) setError(errText(e));
      },
    );
    void look();
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [api, jobId]);

  if (finding) {
    return (
      <Sheet labelledBy="ck-wb-t" role="dialog" onClose={onClose} className="ck-sheet--wb">
        <h2 className="ck-section" id="ck-wb-t">
          What broke the tests?
        </h2>
        {renderResult ? renderResult(finding) : <ResultPlain finding={finding} onClose={onClose} />}
      </Sheet>
    );
  }

  // Running: no onClose, so Escape and a click on the scrim do nothing. Stop is the way out.
  if (job) {
    return (
      <Sheet labelledBy="ck-wb-t" role="dialog" className="ck-sheet--wb">
        <h2 className="ck-section" id="ck-wb-t">
          What broke the tests?
        </h2>
        <p className="ck-sub ck-wb-line" role="status" aria-live="polite">
          {job.line || 'Getting ready…'}
        </p>
        <div className="ck-wb-bar" aria-hidden="true">
          <span />
        </div>
        <p className="ck-wb-muted">Your agents can keep working. This runs at low priority.</p>
        {error && (
          <p className="ck-wb-error" role="alert">
            {error}
          </p>
        )}
        <div className="ck-sheet-actions">
          <button
            type="button"
            className="ck-btn ck-btn--secondary"
            disabled={stopping}
            onClick={() => {
              setStopping(true);
              setError(null);
              api.wbStop(job.id).catch((e) => {
                setStopping(false);
                setError(errText(e));
              });
            }}
          >
            {stopping ? 'Stopping…' : 'Stop'}
          </button>
        </div>
      </Sheet>
    );
  }

  const o = overview;
  const shown = limits ?? o?.limits ?? null;
  const tests = o?.tests ?? [];
  const current = tests.find((t) => t.index === picked) ?? tests[0];
  const showPicker = tests.length === 0 || editing;
  const whyText = !o ? null : !o.can ? (o.whyNot ?? o.busy) : !current ? 'Choose a test command first.' : null;
  const canFind = !!o && o.can && !!current && !starting;

  const declare = async (name: string, commandText: string) => {
    setError(null);
    try {
      const saved = await api.wbDeclare(paneId, name, commandText, DEFAULT_TIMEOUT_MIN);
      setPicked(saved.index);
      setEditing(false);
      setTyped('');
      setOverview(await api.wbOverview(paneId));
    } catch (e) {
      setError(errText(e));
    }
  };

  const find = async () => {
    if (!current || !shown) return;
    setStarting(true);
    setError(null);
    try {
      setJob(await api.wbStart(paneId, current.index, goodId || null, clampLimits(shown), null));
    } catch (e) {
      setError(errText(e));
    } finally {
      setStarting(false);
    }
  };

  const setLimit = (key: keyof Limits, raw: string) => {
    const n = Number(raw);
    if (!shown || raw.trim() === '' || !Number.isFinite(n)) return;
    setLimits(clampLimits({ ...shown, [key]: n }));
  };

  const limitText = shown ? `Stops after ${plural(shown.max_runs, 'run', 'runs')} or ${plural(shown.max_minutes, 'minute', 'minutes')}.` : '';

  return (
    <Sheet labelledBy="ck-wb-t" role="dialog" onClose={onClose} className="ck-sheet--wb">
      <h2 className="ck-section" id="ck-wb-t">
        What broke the tests?
      </h2>
      {!o || !shown ? (
        <p className="ck-sub">Loading…</p>
      ) : (
        <>
          <p className="ck-sub">Puppet Master runs your test command on older save points, in a separate copy. Your folder is not touched.</p>
          {sharedWith.length > 0 && (
            <p className="ck-wb-muted">
              {sharedWith.length === 1
                ? `${sharedWith[0]} also works in this folder. Its turns count too.`
                : `${sharedWith.join(', ')} also work in this folder. Their turns count too.`}
            </p>
          )}
          <div className="ck-wb-rows">
            <div className="ck-wb-row">
              <span className="ck-wb-label" id="ck-wb-cmd-l">
                Test command
              </span>
              {!showPicker && current ? (
                <span className="ck-wb-value">
                  {tests.length > 1 ? (
                    <select
                      className="ck-wb-select"
                      aria-labelledby="ck-wb-cmd-l"
                      value={current.index}
                      onChange={(e) => setPicked(Number(e.target.value))}
                    >
                      {tests.map((t) => (
                        <option key={t.index} value={t.index}>
                          {t.command_text}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <code className="ck-wb-code">{current.command_text}</code>
                  )}
                  <button
                    type="button"
                    className="ck-btn ck-btn--secondary ck-btn--sm"
                    onClick={() => {
                      setTyped(current.command_text);
                      setEditing(true);
                    }}
                  >
                    Change
                  </button>
                </span>
              ) : (
                <span className="ck-wb-muted">{tests.length === 0 ? 'None yet' : ''}</span>
              )}
            </div>
            {showPicker && (
              <div className="ck-wb-picker">
                {o.suggestions.map((s) => (
                  <div className="ck-wb-row" key={s.command_text}>
                    <span>
                      <code className="ck-wb-code">{s.command_text}</code>{' '}
                      <span className="ck-wb-muted">{s.source}</span>
                    </span>
                    <button
                      type="button"
                      className="ck-btn ck-btn--secondary ck-btn--sm"
                      onClick={() => void declare(s.name, s.command_text)}
                    >
                      Use this
                    </button>
                  </div>
                ))}
                <form
                  className="ck-wb-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    const text = typed.trim();
                    if (text) void declare(text, text);
                  }}
                >
                  <input
                    className="ck-wb-input"
                    aria-label="Test command"
                    placeholder="Type a command, like npm test"
                    value={typed}
                    onChange={(e) => setTyped(e.target.value)}
                  />
                  <button type="submit" className="ck-btn ck-btn--secondary ck-btn--sm" disabled={!typed.trim()}>
                    Save
                  </button>
                  {tests.length > 0 && (
                    <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => setEditing(false)}>
                      Cancel
                    </button>
                  )}
                </form>
              </div>
            )}
            {current && current.warnings.length > 0 && <p className="ck-wb-warn">{current.warnings.join(' ')}</p>}
            <div className="ck-wb-row">
              <label className="ck-wb-label" htmlFor="ck-wb-good">
                Tests last passed
              </label>
              <select id="ck-wb-good" className="ck-wb-select" value={goodId} onChange={(e) => setGoodId(e.target.value)}>
                <option value="">Find it for me</option>
                {o.points.map((p) => (
                  <option key={p.checkpointId} value={p.checkpointId}>
                    {p.label}
                  </option>
                ))}
              </select>
            </div>
            {editingLimits ? (
              <div className="ck-wb-limits">
                <label>
                  Runs
                  <input
                    className="ck-wb-num"
                    type="number"
                    min={1}
                    max={30}
                    value={shown.max_runs}
                    onChange={(e) => setLimit('max_runs', e.target.value)}
                  />
                </label>
                <label>
                  Minutes
                  <input
                    className="ck-wb-num"
                    type="number"
                    min={1}
                    max={60}
                    value={shown.max_minutes}
                    onChange={(e) => setLimit('max_minutes', e.target.value)}
                  />
                </label>
                <button type="button" className="ck-wb-link" onClick={() => setEditingLimits(false)}>
                  Done
                </button>
              </div>
            ) : (
              <p className="ck-wb-muted">
                {limitText}{' '}
                <button type="button" className="ck-wb-link" onClick={() => setEditingLimits(true)}>
                  Change
                </button>
              </p>
            )}
          </div>
          {error && (
            <p className="ck-wb-error" role="alert">
              {error}
            </p>
          )}
          {whyText && <p className="ck-wb-muted">{whyText}</p>}
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={onClose}>
              Cancel
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={!canFind} onClick={() => void find()}>
              {starting ? 'Starting…' : 'Find it'}
            </button>
          </div>
        </>
      )}
    </Sheet>
  );
}
