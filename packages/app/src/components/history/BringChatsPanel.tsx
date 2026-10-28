// "Bring my old chats": finds the chats each agent kept on this PC and adds them to search. It is the content
// only; the caller wraps it in a Sheet (Escape and Tab come from there). Nothing in an agent folder is read
// until the person presses "Find my chats". Nothing leaves this computer.
import { useEffect, useRef, useState } from 'react';
import { chatIndexSetEnabled } from '../../lib/chatIndex';
import {
  chatCount,
  chatImportApi,
  DEFAULT_SINCE_DAYS,
  foundLine,
  idleProgress,
  onImportProgress,
  progressLine,
  SINCE_CHOICES,
  type ImportApi,
  type ImportFound,
  type ImportProgress,
  type ImportState,
} from '../../lib/chatImport';
import '../../styles/history.css';

export const BRING_CHATS_TITLE_ID = 'bc-panel-title';
/** The text of the "Remove imported chats" question. The Settings row uses the same words. */
export const BRING_REMOVE_BODY = "This removes the old chats Puppet Master added to search. Your agents' own chat files are not touched.";

type View = 'intro' | 'off' | 'looking' | 'found' | 'reading' | 'result';

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Looking and reading come first; then the index switch; then the list; then the last run's result. */
function pickView(a: { looking: boolean; starting: boolean; progress: ImportProgress; indexEnabled: boolean; hasFound: boolean }): View {
  if (a.looking) return 'looking';
  if (a.starting || a.progress.phase === 'reading') return 'reading';
  if (!a.indexEnabled) return 'off';
  if (a.hasFound) return 'found';
  if (a.progress.phase === 'done' || a.progress.phase === 'cancelled' || a.progress.phase === 'failed') return 'result';
  return 'intro';
}

/**
 * `startLooking`: the caller's own "Find my chats" press already asked the person (the first-start card), so the
 * look starts once, when the saved answers are in. Without it the intro waits for its own button.
 */
export function BringChatsPanel({ api = chatImportApi, onClose, startLooking = false }: { api?: ImportApi; onClose?: () => void; startLooking?: boolean }) {
  const [state, setState] = useState<ImportState | null>(null);
  const [progress, setProgress] = useState<ImportProgress>(idleProgress);
  const [found, setFound] = useState<ImportFound[] | null>(null);
  // The switches: which agents are on. Set when a look finishes, from the saved choice or the default.
  const [picked, setPicked] = useState<Record<string, boolean>>({});
  const [sinceDays, setSinceDays] = useState<number | null>(DEFAULT_SINCE_DAYS);
  const [looking, setLooking] = useState(false);
  const [starting, setStarting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Reads the saved settings and the last run only. Also listens for progress from the worker.
  useEffect(() => {
    let live = true;
    void api
      .state()
      .then((next) => {
        if (!live) return;
        setState(next);
        setSinceDays(next.sinceDays);
        setProgress(next.progress);
      })
      .catch((e: unknown) => live && setError(errorText(e)));
    const stop = onImportProgress((next) => live && setProgress(next));
    return () => {
      live = false;
      stop();
    };
  }, [api]);

  const indexEnabled = state?.indexEnabled ?? true;
  const view = pickView({ looking, starting, progress, indexEnabled, hasFound: found !== null });

  const labelOf = (agent: string | null): string | undefined => {
    if (!agent) return undefined;
    return found?.find((f) => f.id === agent)?.label ?? state?.agents.find((a) => a.id === agent)?.label;
  };

  const look = async (days: number | null) => {
    setError(null);
    setNotice(null);
    setLooking(true);
    try {
      const list = await api.look(days);
      setFound(list);
      setPicked(
        Object.fromEntries(
          list.map((f) => [f.id, state?.agents.find((a) => a.id === f.id)?.enabled ?? !f.fromDefinition]),
        ),
      );
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLooking(false);
    }
  };

  const chooseSince = (days: number | null) => {
    setSinceDays(days);
    void look(days);
  };

  // Once per panel: the caller's press was the consent. Waits for the saved window and switches (`state`).
  const startedLook = useRef(false);
  useEffect(() => {
    if (!startLooking || !state || startedLook.current) return;
    startedLook.current = true;
    void look(state.sinceDays);
    // `look` is rebuilt every render; this must run once, when `state` arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startLooking, state]);

  const enabledIds = (found ?? []).filter((f) => picked[f.id]).map((f) => f.id);
  // Chats already in search are not counted again.
  const addCount = (found ?? []).filter((f) => picked[f.id]).reduce((sum, f) => sum + Math.max(0, f.chats - f.already), 0);

  const start = async () => {
    setError(null);
    setNotice(null);
    setStarting(true);
    try {
      const next = await api.start(enabledIds, sinceDays);
      setProgress(next);
      setFound(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStarting(false);
    }
  };

  const stop = async () => {
    setError(null);
    try {
      setProgress(await api.cancel());
    } catch (e) {
      setError(errorText(e));
    }
  };

  const turnOn = async () => {
    setBusy(true);
    setError(null);
    try {
      const status = await chatIndexSetEnabled(true);
      setState((prev) => prev && { ...prev, indexEnabled: status.enabled });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setConfirming(false);
    setBusy(true);
    setError(null);
    try {
      const status = await api.clear();
      setState((prev) => prev && { ...prev, imported: { chats: status.importedChats ?? 0, items: status.importedItems ?? 0 } });
      setProgress(idleProgress());
      setFound(null);
      setNotice("Old chats removed from search. Your agents' own chat files are not touched.");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const toggle = (id: string) => setPicked((prev) => ({ ...prev, [id]: !prev[id] }));

  const closeButton = onClose ? (
    <button type="button" className="ck-btn ck-btn--plain" onClick={onClose}>
      Close
    </button>
  ) : null;

  const counters = (
    <>
      {progress.unchanged > 0 && <span className="bc-counts">Skipped {progress.unchanged} (already in search)</span>}
      {progress.tooBig > 0 && <span className="bc-counts">Too big {progress.tooBig}</span>}
      {progress.unreadable > 0 && <span className="bc-counts">Could not read {progress.unreadable}</span>}
    </>
  );

  const added = Math.max(0, progress.chatsDone - progress.unchanged);
  const greyed = found === null ? [] : (state?.agents ?? []).filter((a) => !found.some((f) => f.id === a.id));

  return (
    <section className="bc-panel" aria-labelledby={BRING_CHATS_TITLE_ID}>
      <h2 className="ck-section" id={BRING_CHATS_TITLE_ID}>
        Bring my old chats
      </h2>
      {error && (
        <p className="bc-error" role="alert">
          {error}
        </p>
      )}
      {notice && view !== 'reading' && (
        <p className="ck-sub" role="status">
          {notice}
        </p>
      )}

      {view === 'intro' && (
        <>
          <p className="ck-sub">Read my old chats on this PC. Nothing leaves this computer.</p>
          <p className="ck-sub">Secrets are hidden before anything is saved. Tool output is not saved. You can remove it all in one click.</p>
          <div className="bc-actions">
            {closeButton}
            <button type="button" className="ck-btn ck-btn--primary" onClick={() => void look(sinceDays)}>
              Find my chats
            </button>
          </div>
        </>
      )}

      {view === 'off' && (
        <>
          <p className="ck-sub">Chat search is off. Turn it on to bring your old chats.</p>
          <div className="bc-actions">
            {closeButton}
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void turnOn()}>
              Turn on
            </button>
          </div>
        </>
      )}

      {view === 'looking' && (
        <p className="ck-sub" role="status">
          Looking…
        </p>
      )}

      {view === 'found' && found && (
        <>
          {found.length === 0 && greyed.length === 0 && <p className="ck-sub">No agent has old chats to read.</p>}
          <ul className="bc-agents" aria-label="Agents">
            {found.map((f) => {
              const on = Boolean(picked[f.id]);
              return (
                <li key={f.id} className={`bc-agent${on ? '' : ' is-off'}`}>
                  <div className="bc-agent-main">
                    <div className="bc-agent-name">{foundLine(f)}</div>
                    {f.places.length > 0 && <div className="bc-agent-sub">Reads {f.places.join(', ')}</div>}
                    {f.fromDefinition && <div className="bc-agent-sub">From your agent definition</div>}
                    {f.already > 0 && <div className="bc-agent-sub">{f.already.toLocaleString('en-US')} already in search</div>}
                  </div>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={on}
                    aria-label={`Add ${f.label} chats`}
                    className="ck-switch"
                    onClick={() => toggle(f.id)}
                  >
                    <span className="ck-switch-knob" />
                  </button>
                </li>
              );
            })}
            {greyed.map((a) => (
              <li key={a.id} className="bc-agent is-off">
                <div className="bc-agent-main">
                  <div className="bc-agent-name">{a.label}</div>
                  <div className="bc-agent-sub">Can't read its old chats yet</div>
                </div>
              </li>
            ))}
          </ul>
          <label className="bc-since">
            <span>Only chats from the last</span>
            <select
              aria-label="Only chats from the last"
              value={sinceDays === null ? 'all' : String(sinceDays)}
              onChange={(e) => chooseSince(e.target.value === 'all' ? null : Number(e.target.value))}
            >
              {SINCE_CHOICES.map((c) => (
                <option key={c.label} value={c.days === null ? 'all' : String(c.days)}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
          {enabledIds.length === 0 && <p className="ck-sub">Turn on at least one agent to add its chats.</p>}
          <div className="bc-actions">
            {closeButton}
            <button type="button" className="ck-btn ck-btn--primary" disabled={enabledIds.length === 0 || addCount === 0} onClick={() => void start()}>
              {`Add ${chatCount(addCount)} to search`}
            </button>
          </div>
        </>
      )}

      {view === 'reading' && (
        <>
          <p className="ck-sub" role="status">
            {progress.phase === 'idle' ? 'Starting…' : progressLine(progress, labelOf(progress.agent))}
          </p>
          <div
            className="bc-bar"
            role="progressbar"
            aria-label="Reading old chats"
            aria-valuemin={0}
            aria-valuemax={Math.max(progress.chatsTotal, 1)}
            aria-valuenow={progress.chatsDone}
          >
            <span className="bc-bar-fill" style={{ width: `${progress.chatsTotal > 0 ? Math.min(100, (progress.chatsDone / progress.chatsTotal) * 100) : 0}%` }} />
          </div>
          {counters}
          <div className="bc-actions">
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void stop()}>
              Stop
            </button>
          </div>
        </>
      )}

      {view === 'result' && (
        <>
          {progress.phase === 'done' ? (
            <p className="ck-sub" role="status">
              {added > 0
                ? `Added ${chatCount(added)}. Press Ctrl+Shift+F to find anything in them.`
                : 'Nothing new to add. Your old chats are already in search.'}
            </p>
          ) : (
            <p className="ck-sub" role="status">
              {progressLine(progress, labelOf(progress.agent))}
            </p>
          )}
          {counters}
          {confirming && (
            <>
              <p className="ck-sub">{BRING_REMOVE_BODY}</p>
              <div className="bc-actions">
                <button type="button" className="ck-btn ck-btn--secondary" onClick={() => setConfirming(false)}>
                  Cancel
                </button>
                <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void remove()}>
                  Remove
                </button>
              </div>
            </>
          )}
          {!confirming && (
            <div className="bc-actions">
              <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => setConfirming(true)}>
                Remove imported chats
              </button>
              <button type="button" className="ck-btn ck-btn--secondary" disabled={busy || looking} onClick={() => void look(sinceDays)}>
                Look again
              </button>
              {onClose && (
                <button type="button" className="ck-btn ck-btn--primary" onClick={onClose}>
                  Done
                </button>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
