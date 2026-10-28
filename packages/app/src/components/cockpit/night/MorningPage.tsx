// The Morning page: what Night Shift did while the person slept. A full overlay like "Your plans this month".
// It reads the night with `useNight()`, calls `night_seen` once when it opens (that holds the morning push),
// and runs Keep and Throw away one task at a time, or Keep all that pass. Agent text is shown as text only.
import { useEffect, useRef, useState } from 'react';
import { type NightTaskView, type NightView, nightApi, nightError, useNight } from '../../../lib/night';
import { CONFIRM_MS, TaskRow, plural } from './TaskRow';
import '../../../styles/night-morning.css';

export type MorningGroupKey = 'ready' | 'needs_look' | 'didnt_run' | 'no_changes' | 'in_progress';

export interface MorningGroup {
  key: MorningGroupKey;
  title: string;
  tasks: NightTaskView[];
}

const GROUP_TITLES: Array<[MorningGroupKey, string]> = [
  ['ready', 'READY TO KEEP'],
  ['needs_look', 'NEEDS A LOOK'],
  ['didnt_run', "DIDN'T RUN"],
  ['no_changes', 'MADE NO CHANGES'],
  ['in_progress', 'IN PROGRESS'],
];

/** Which group a task belongs to. A task still going (or queued or waiting) is in progress. */
export function groupOf(task: NightTaskView): MorningGroupKey {
  if (task.state !== 'finished') return 'in_progress';
  switch (task.class) {
    case 'ready':
    case 'no_checks':
      return 'ready';
    case 'didnt_run':
      return 'didnt_run';
    case 'no_changes':
      return 'no_changes';
    default:
      return 'needs_look';
  }
}

/** The groups in order, each with its tasks in the person's order. Empty groups are left out. */
export function groupTasks(tasks: NightTaskView[]): MorningGroup[] {
  return GROUP_TITLES.map(([key, title]) => ({ key, title, tasks: tasks.filter((t) => groupOf(t) === key) })).filter(
    (group) => group.tasks.length > 0,
  );
}

/** What "Keep all that pass" keeps: finished Ready copies that nobody has kept or thrown away yet. */
export function passingTasks(tasks: NightTaskView[]): NightTaskView[] {
  return tasks.filter((t) => t.state === 'finished' && t.class === 'ready' && t.review === 'pending' && t.canKeep);
}

export interface MorningNotice {
  text: string;
  /** Titles to list under the text (for example the ones still to keep). */
  titles: string[];
}

/** The line after Keep all: how many were kept, whether a conflict stopped it, and what is still to keep. */
export function keepAllNotice(before: NightTaskView[], after: NightTaskView[]): MorningNotice {
  const now = new Map(after.map((t) => [t.id, t]));
  const kept = before.filter((t) => now.get(t.id)?.review === 'kept').length;
  const conflict = before.some((t) => now.get(t.id)?.review === 'keep_failed');
  const left = before.filter((t) => now.get(t.id)?.review === 'pending').map((t) => t.title);
  let text = `Kept ${kept} of ${before.length}.`;
  if (conflict) text += ' Stopped at a conflict.';
  if (left.length > 0) text += ' Still to keep:';
  return { text, titles: left };
}

/** The newer of two views: an action's answer, or the night as it was last read. */
function newest(a: NightView | null, b: NightView | null): NightView | null {
  if (!a) return b;
  if (!b) return a;
  return b.rev > a.rev ? b : a;
}

function whenText(run: NightView): string {
  return run.to ? `Night Shift  ${run.from} to ${run.to}` : `Night Shift  started ${run.from}`;
}

function totalsText(run: NightView): string {
  const plans = run.byPlan.map((p) => `${p.label} ${plural(p.tasks, 'task')}`);
  return [plural(run.turns, 'turn'), run.duration, ...plans].filter(Boolean).join(' · ');
}

export const EMPTY_TEXT = 'No Night Shift yet. Plan tonight from the + menu.';
const LATEST = 'latest';
const ALL = 'all';

export interface MorningPageProps {
  onClose: () => void;
  /** Opens the terminal of a task's last try. */
  onOpenPane?: (paneId: string) => void;
}

export function MorningPage({ onClose, onOpenPane }: MorningPageProps) {
  const { state, error: readError } = useNight();
  // The newest view an action returned. The page shows it until the night's own state is newer.
  const [fresh, setFresh] = useState<NightView | null>(null);
  const [picked, setPicked] = useState(LATEST);
  // The task id (or ALL) that has a request in flight.
  const [pending, setPending] = useState<string | null>(null);
  const [rowErrors, setRowErrors] = useState<Record<string, string>>({});
  const [askAll, setAskAll] = useState(false);
  const [notice, setNotice] = useState<MorningNotice | null>(null);
  const seen = useRef(false);

  // Opening the page counts as seeing it: the held morning push is then not sent. Once per page.
  useEffect(() => {
    if (seen.current) return;
    seen.current = true;
    nightApi.seen().catch(() => {});
  }, []);

  // "Keep all that pass" waits for its second press, then goes back to the button.
  useEffect(() => {
    if (!askAll) return;
    const timer = window.setTimeout(() => setAskAll(false), CONFIRM_MS);
    return () => window.clearTimeout(timer);
  }, [askAll]);

  const run = newest(fresh, state?.run ?? null);
  const history = (state?.history ?? []).filter((row) => row.id !== run?.id);
  const older = picked === LATEST ? null : (history.find((row) => row.id === picked) ?? null);
  const tasks = run?.tasks ?? [];
  const groups = groupTasks(tasks);
  const passing = passingTasks(tasks);
  const live =
    run?.state === 'running'
      ? 'Running. Each task shows here as it finishes.'
      : run?.state === 'scheduled'
        ? `Starts at ${run.from}.`
        : null;

  async function change(id: string, request: () => Promise<NightView>) {
    setPending(id);
    try {
      setFresh(await request());
      setRowErrors((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    } catch (err) {
      const message = nightError(err).message;
      setRowErrors((prev) => ({ ...prev, [id]: message }));
    } finally {
      setPending(null);
    }
  }

  function keepOne(task: NightTaskView) {
    void change(task.id, () => nightApi.keep(task.id));
  }

  function throwOne(task: NightTaskView) {
    void change(task.id, () => nightApi.throwAway(task.id));
  }

  async function keepAll() {
    setAskAll(false);
    setPending(ALL);
    try {
      const next = await nightApi.keepAllReady();
      setFresh(next);
      setNotice(keepAllNotice(passing, next.tasks));
    } catch (err) {
      setNotice({ text: nightError(err).message, titles: [] });
    } finally {
      setPending(null);
    }
  }

  async function savePage() {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({ defaultPath: 'night-shift.html', filters: [{ name: 'Web page', extensions: ['html'] }] });
      if (!path) return;
      await nightApi.reportSave(path);
      setNotice({ text: 'Saved the page.', titles: [] });
    } catch (err) {
      setNotice({ text: nightError(err).message, titles: [] });
    }
  }

  return (
    <div className="ck-morning">
      <header className="ck-morning-head">
        <div>
          <h1 className="ck-title">Morning</h1>
          {run && <p className="ck-sub">{whenText(run)}</p>}
        </div>
        <div className="ck-morning-head-actions">
          <button type="button" className="ck-btn ck-btn--plain" disabled={!run} onClick={() => void savePage()}>
            Save as page
          </button>
          <button type="button" className="ck-btn ck-btn--plain" onClick={onClose}>
            Close
          </button>
        </div>
      </header>

      {readError && (
        <p className="ck-morning-error" role="alert">
          {readError}
        </p>
      )}

      {history.length > 0 && (
        <select
          aria-label="Night"
          className="ck-morning-select"
          value={picked}
          onChange={(e) => setPicked(e.target.value)}
        >
          <option value={LATEST}>{run ? 'Latest night' : 'Latest'}</option>
          {history.map((row) => (
            <option key={row.id} value={row.id}>
              {row.line}
            </option>
          ))}
        </select>
      )}

      {state === null && !readError && <p className="ck-morning-empty">Loading</p>}

      {older ? (
        <section className="ck-morning-older">
          <p className="ck-morning-summary">{older.line}</p>
          <p className="ck-morning-empty">Only the summary of an older night is shown here.</p>
        </section>
      ) : run === null ? (
        state !== null && <p className="ck-morning-empty">{EMPTY_TEXT}</p>
      ) : (
        <>
          <p className="ck-morning-summary">{run.line}</p>
          <p className="ck-morning-totals">{totalsText(run)}</p>
          {live && <p className="ck-morning-live">{live}</p>}

          {passing.length > 0 &&
            (askAll ? (
              <div className="ck-morning-ask-all">
                <p className="ck-morning-ask" role="status">
                  Keep these {passing.length} in list order:
                </p>
                <ul className="ck-morning-titles">
                  {passing.map((t) => (
                    <li key={t.id}>{t.title}</li>
                  ))}
                </ul>
                <button
                  type="button"
                  className="ck-btn ck-btn--primary"
                  disabled={pending !== null}
                  onClick={() => void keepAll()}
                >
                  Keep all
                </button>
              </div>
            ) : (
              <button
                type="button"
                className="ck-btn ck-btn--primary"
                disabled={pending !== null}
                onClick={() => setAskAll(true)}
              >
                Keep all that pass ({passing.length})
              </button>
            ))}

          {notice && (
            <div className="ck-morning-notice" role="status">
              <p>{notice.text}</p>
              {notice.titles.length > 0 && (
                <ul className="ck-morning-titles">
                  {notice.titles.map((title, i) => (
                    <li key={i}>{title}</li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {groups.map((group) => (
            <section key={group.key} className="ck-morning-group">
              <h2 className="ck-morning-group-title">
                {group.title} ({group.tasks.length})
              </h2>
              <ul className="ck-morning-list">
                {group.tasks.map((task) => (
                  <TaskRow
                    key={task.id}
                    task={task}
                    busy={pending === task.id || pending === ALL}
                    error={rowErrors[task.id] ?? null}
                    onKeep={keepOne}
                    onThrowAway={throwOne}
                    onOpenPane={onOpenPane}
                  />
                ))}
              </ul>
            </section>
          ))}

          {run.events.length > 0 && (
            <section className="ck-morning-group">
              <h2 className="ck-morning-group-title">THE NIGHT</h2>
              <ol className="ck-morning-events">
                {run.events.map((event, i) => (
                  <li key={`${event.at}-${i}`}>
                    <span className="ck-morning-time">{event.at}</span>
                    <span className="ck-morning-glyph" aria-hidden="true">
                      {event.glyph}
                    </span>
                    <span>{event.text}</span>
                  </li>
                ))}
              </ol>
            </section>
          )}
        </>
      )}
    </div>
  );
}
