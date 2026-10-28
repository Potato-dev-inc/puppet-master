// "Tonight" (SPEC n32 3.10): the sheet where a person plans the night. Up to 10 tasks, each with its own folder, words,
// plan, tests and budget; then the plans to use, the stop time, and the checks before Start. "Start Night Shift" is off
// with one plain line that says why (`startBlockedReason`). The desktop checks everything again, so its refusal text
// is shown when Start fails. Nothing here merges anything.
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  nightApi,
  nightError,
  nightErrorText,
  startBlockedReason,
  type NightChecksView,
  type NightPlanView,
  type NightSettingsView,
  type NightStartInput,
  type NightStateView,
  type NightView,
} from '../../../lib/night';
import { Sheet } from '../Sheet';
import { PlansPicker, agentOf } from './PlansPicker';
import { TestsPopover } from './TestsPopover';
import '../../../styles/cockpit-changes.css';
import '../../../styles/night.css';

/** A folder the sheet can offer. `git` means it is a git project with a commit (a separate copy can be made). */
export interface TonightProject {
  path: string;
  name: string;
  git: boolean;
}

// The limits of `night_shift/model.rs` and `lib/night.ts` (the sheet refuses the same values before Start).
const MAX_TASKS = 10;
const MAX_PARALLEL = 2;
const TASK_TEXT_MAX = 2000;
const MIN_TURNS = 3;
const MAX_TURNS = 40;
const MIN_MINUTES = 20;
const MAX_MINUTES = 240;

/** The words of the keep-awake confirm (SPEC n32 3.9). Shown once; `awakeAsked` is saved after it. */
const AWAKE_WORDS =
  'Puppet Master will stop this computer from sleeping until Night Shift ends, then let it sleep again. Closing the lid or pressing the power button still sleeps it. Keep it plugged in.';

/** One task row as the sheet edits it. `who` is `any` or one plan key. */
interface Draft {
  id: number;
  project: string;
  text: string;
  who: string;
  mayChangeTests: boolean;
  turns: number;
  minutes: number;
  /** The "More" part is open. */
  more: boolean;
  /** The person chose "No tests for this project". */
  noTests: boolean;
}

const plural = (n: number, one: string, many: string): string => (n === 1 ? one : many);

/** "A", "A and B", "A, B and C". */
function joinNames(names: string[]): string {
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** The first local `HH:MM` at or after `fromMs`, or null when the text is not a time. Same rule as the desktop. */
function localAt(fromMs: number, hhmm: string): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(hhmm);
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) return null;
  const from = new Date(fromMs);
  for (let days = 0; days < 2; days += 1) {
    const at = new Date(from.getFullYear(), from.getMonth(), from.getDate() + days, hour, minute).getTime();
    if (at >= fromMs) return at;
  }
  return null;
}

/** "tomorrow" when the stop time falls on a later day than the start, else nothing. */
function stopDayWord(nowMs: number, startNow: boolean, startHhmm: string, stopHhmm: string): string {
  const start = startNow ? nowMs : localAt(nowMs, startHhmm);
  if (start === null) return '';
  const stop = localAt(start, stopHhmm);
  if (stop === null) return '';
  return new Date(stop).toDateString() === new Date(start).toDateString() ? '' : 'tomorrow';
}

const clampParallel = (n: number): number => Math.min(MAX_PARALLEL, Math.max(1, Math.round(n) || 1));

function Switch({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className="ck-switch"
      disabled={disabled}
      onClick={() => onChange(!on)}
    >
      <span className="ck-switch-knob" />
    </button>
  );
}

export function TonightSheet({
  projects,
  undoOn = true,
  now = Date.now,
  onClose,
  onStarted,
  onNotify,
}: {
  projects: TonightProject[];
  /** Undo is on for the folders. Off blocks Start, as the desktop does. Default: on. */
  undoOn?: boolean;
  /** The clock, epoch ms. Default: now. */
  now?: () => number;
  onClose: () => void;
  /** Called with the run the desktop made, before the sheet closes. */
  onStarted?: (run: NightView) => void;
  /** A plain line for the toast, such as "Night Shift started. It stops at 07:00." */
  onNotify?: (text: string) => void;
}) {
  // Git folders first; the rest stay listed (Start says why they cannot be used).
  const ordered = useMemo(() => [...projects.filter((p) => p.git), ...projects.filter((p) => !p.git)], [projects]);
  const gitOf = (path: string): boolean => projects.find((p) => p.path === path)?.git ?? false;

  const [state, setState] = useState<NightStateView | null>(null);
  const [plans, setPlans] = useState<NightPlanView[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [checks, setChecks] = useState<Record<string, NightChecksView>>({});
  const [tasks, setTasks] = useState<Draft[]>([]);
  const [testsFor, setTestsFor] = useState<number | null>(null);
  const [ticked, setTicked] = useState<string[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [stopHhmm, setStopHhmm] = useState('07:00');
  const [startNow, setStartNow] = useState(true);
  const [startHhmm, setStartHhmm] = useState('23:30');
  const [parallel, setParallel] = useState(1);
  const [keepAwake, setKeepAwake] = useState(false);
  const [awakeAsked, setAwakeAsked] = useState(false);
  const [confirmAwake, setConfirmAwake] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ids = useRef(0);

  const makeTask = (project: string, s: NightSettingsView): Draft => ({
    id: (ids.current += 1),
    project,
    text: '',
    who: 'any',
    mayChangeTests: false,
    turns: s.turns,
    minutes: s.minutes,
    more: false,
    noTests: false,
  });

  // The saved settings and the first task, read once when the sheet opens.
  useEffect(() => {
    let live = true;
    nightApi
      .state()
      .then((next) => {
        if (!live) return;
        setState(next);
        setStopHhmm(next.settings.stopHhmm);
        setParallel(clampParallel(next.settings.parallel));
        setTicked(next.settings.plans);
        setAwakeAsked(next.settings.awakeAsked);
        setTasks((cur) => (cur.length ? cur : [makeTask(ordered[0]?.path ?? '', next.settings)]));
      })
      .catch((err) => live && setLoadError(nightErrorText(err)));
    return () => {
      live = false;
    };
    // Read once: the sheet is opened fresh each time.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The folders in use decide which plans are listed (their "tried here" hint and whether each runs there).
  const paths = useMemo(() => [...new Set(tasks.map((t) => t.project).filter((p) => p !== ''))], [tasks]);
  const pathsKey = paths.join('\n');
  useEffect(() => {
    let live = true;
    nightApi
      .plans(paths)
      .then((list) => {
        if (!live) return;
        setPlans(list);
        setTicked((cur) => cur.filter((key) => list.some((plan) => plan.key === key)));
      })
      .catch((err) => live && setLoadError(nightErrorText(err)));
    return () => {
      live = false;
    };
    // Only the set of folders matters; `paths` is derived from it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathsKey]);

  // Each folder's tests, read once per folder (the popover saves its own changes).
  useEffect(() => {
    let live = true;
    for (const path of paths) {
      nightApi
        .checksGet(path)
        .then((view) => live && setChecks((cur) => (cur[path] ? cur : { ...cur, [path]: view })))
        .catch(() => {});
    }
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathsKey]);

  const viewOf = (path: string): NightChecksView | null => checks[path] ?? null;
  const labelOf = (key: string): string => plans?.find((plan) => plan.key === key)?.label ?? key;
  const loaded = state !== null && plans !== null;
  const busyRun = state?.run?.state === 'running' || state?.run?.state === 'scheduled';

  const input: NightStartInput = {
    tasks: tasks.map((t) => ({
      project: t.project,
      text: t.text,
      who: t.who === 'any' ? null : t.who,
      mayChangeTests: t.mayChangeTests,
      turns: t.turns,
      minutes: t.minutes,
    })),
    plans: ticked,
    stopHhmm,
    startHhmm: startNow ? null : startHhmm,
    parallel,
    keepAwake,
  };
  const blocked =
    state && plans
      ? startBlockedReason({
          enabled: state.enabled,
          busy: !!busyRun,
          undoOn,
          isGit: gitOf,
          plans,
          awakeAsked,
          input,
          nowMs: now(),
        })
      : null;
  const shownBlock = blocked?.message ?? null;
  const disabled = !loaded || blocked !== null || busy;

  // Worst case, in the words of the spec: "3 tasks x 12 turns = 36 turns, at most 2 at a time, until 07:00."
  const n = tasks.length;
  const turnTotal = tasks.reduce((sum, t) => sum + t.turns, 0);
  const sameTurns = tasks.every((t) => t.turns === tasks[0]?.turns);
  const worst =
    n > 0
      ? `Worst case: ${
          sameTurns
            ? `${n} ${plural(n, 'task', 'tasks')} x ${tasks[0].turns} ${plural(tasks[0].turns, 'turn', 'turns')} = ${turnTotal} ${plural(turnTotal, 'turn', 'turns')}`
            : `${n} tasks, ${turnTotal} turns in all`
        }, at most ${Math.min(parallel, n)} at a time, until ${stopHhmm}.`
      : null;

  const gitOk = n > 0 && tasks.every((t) => t.project !== '' && gitOf(t.project));
  const untested = tasks.filter((t) => t.project !== '' && viewOf(t.project)?.declared.length === 0).length;
  const stopWord = stopDayWord(now(), startNow, startHhmm, stopHhmm);

  const patch = (id: number, change: Partial<Draft>) =>
    setTasks((cur) => cur.map((t) => (t.id === id ? { ...t, ...change } : t)));
  const addTask = () => {
    if (!state || tasks.length >= MAX_TASKS) return;
    const last = tasks[tasks.length - 1];
    setTasks((cur) => [...cur, makeTask(last?.project ?? ordered[0]?.path ?? '', state.settings)]);
  };
  const removeTask = (id: number) => {
    setTasks((cur) => cur.filter((t) => t.id !== id));
    setTestsFor((open) => (open === id ? null : open));
  };
  const moveTask = (index: number, delta: number) =>
    setTasks((cur) => {
      const target = index + delta;
      if (target < 0 || target >= cur.length) return cur;
      const next = [...cur];
      [next[index], next[target]] = [next[target], next[index]];
      return next;
    });

  /** Ticking a second login of one agent unticks the first; a task on an unticked plan goes back to Any. */
  const togglePlan = (key: string, on: boolean) => {
    setNotice(null);
    if (!on) {
      setTicked((list) => list.filter((k) => k !== key));
      setTasks((cur) => cur.map((t) => (t.who === key ? { ...t, who: 'any' } : t)));
      return;
    }
    const agent = agentOf(key);
    const clash = ticked.filter((k) => k !== key && agentOf(k) === agent);
    setTicked((list) => [...list.filter((k) => agentOf(k) !== agent), key]);
    if (clash.length > 0) {
      setNotice("Puppet Master won't switch between two logins of one company.");
      setTasks((cur) => cur.map((t) => (clash.includes(t.who) ? { ...t, who: 'any' } : t)));
    }
  };

  const settingsNow = (): NightSettingsView => ({
    plans: ticked,
    stopHhmm,
    parallel,
    turns: state?.settings.turns ?? 12,
    minutes: state?.settings.minutes ?? 90,
    awakeAsked,
  });

  const awakeToggle = (on: boolean) => {
    if (!on) {
      setKeepAwake(false);
      return;
    }
    if (awakeAsked) setKeepAwake(true);
    else setConfirmAwake(true);
  };

  /** The one-time confirm. Either answer saves that it was asked; only "Keep it awake" turns the switch on. */
  const answerAwake = async (keep: boolean) => {
    setError(null);
    try {
      await nightApi.settingsSet({ ...settingsNow(), awakeAsked: true });
      setAwakeAsked(true);
      setKeepAwake(keep);
      setConfirmAwake(false);
    } catch (err) {
      setError(nightErrorText(err));
    }
  };

  const start = async () => {
    if (disabled) return;
    setBusy(true);
    setError(null);
    try {
      const run = await nightApi.start(input);
      onStarted?.(run);
      void nightApi.settingsSet(settingsNow()).catch(() => {});
      onNotify?.(`Night Shift started. It stops at ${stopHhmm}.`);
      onClose();
    } catch (err) {
      setError(nightError(err).message);
      setBusy(false);
    }
  };

  /** The tests chip's words: the declared commands, or "No tests" when the person chose it. */
  const testsText = (t: Draft): string => {
    const declared = t.project ? (viewOf(t.project)?.declared ?? []) : [];
    if (declared.length > 0) return declared.map((check) => check.name).join(', ');
    return t.noTests ? 'No tests' : 'Pick tests…';
  };

  /** The one line under a task: no tests, first time with the chosen plan, or who has worked here before. */
  const noteOf = (t: Draft): string | null => {
    if (t.project === '') return null;
    if (viewOf(t.project)?.declared.length === 0) return 'No tests declared. It will run, but nothing will be checked.';
    if (t.who !== 'any') {
      const plan = plans?.find((p) => p.key === t.who);
      if (!plan) return null;
      return plan.triedHere?.[t.project]
        ? `${plan.label} has worked here before.`
        : `First time with ${plan.label} here: it may ask you to trust the folder.`;
    }
    const tried = ticked
      .map((key) => plans?.find((p) => p.key === key))
      .filter((p): p is NightPlanView => !!p && !!p.triedHere?.[t.project])
      .map((p) => p.label);
    return tried.length > 0 ? `${joinNames(tried)} ${tried.length === 1 ? 'has' : 'have'} worked here before.` : null;
  };

  return (
    <Sheet labelledBy="ck-night-tonight-t" role="dialog" className="ck-night-sheet" onClose={busy ? undefined : onClose}>
      <div className="ck-night-head">
        <h2 className="ck-section" id="ck-night-tonight-t">
          Tonight
        </h2>
        <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" aria-label="Close" disabled={busy} onClick={onClose}>
          ×
        </button>
      </div>
      <p className="ck-night-intro">
        Puppet Master works through these while you sleep. Each task has its own separate copy. Nothing is merged until you
        press Keep.
      </p>

      {loadError && (
        <p className="ck-night-error" role="alert">
          {loadError}
        </p>
      )}

      <ul className="ck-night-tasks" aria-label="Tasks">
        {tasks.map((t, i) => {
          const num = i + 1;
          const note = noteOf(t);
          return (
            <li key={t.id} className="ck-night-task">
              <div className="ck-night-task-head">
                <span className="ck-night-n" aria-hidden="true">
                  {num}
                </span>
                <select
                  className="ck-night-select"
                  aria-label={`Project for task ${num}`}
                  value={t.project}
                  onChange={(e) => patch(t.id, { project: e.target.value, noTests: false })}
                >
                  {t.project === '' && <option value="">Pick a project…</option>}
                  {ordered.map((p) => (
                    <option key={p.path} value={p.path}>
                      {p.git ? p.name : `${p.name} (not a git project)`}
                    </option>
                  ))}
                </select>
                <textarea
                  className="ck-night-text"
                  aria-label={`Task ${num}`}
                  rows={2}
                  maxLength={TASK_TEXT_MAX}
                  placeholder="What should it do?"
                  value={t.text}
                  onChange={(e) => patch(t.id, { text: e.target.value })}
                />
                <span className="ck-night-moves">
                  <button
                    type="button"
                    className="ck-btn ck-btn--plain ck-btn--sm"
                    aria-label={`Move task ${num} up`}
                    disabled={i === 0}
                    onClick={() => moveTask(i, -1)}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    className="ck-btn ck-btn--plain ck-btn--sm"
                    aria-label={`Move task ${num} down`}
                    disabled={i === tasks.length - 1}
                    onClick={() => moveTask(i, 1)}
                  >
                    ↓
                  </button>
                  <button
                    type="button"
                    className="ck-btn ck-btn--plain ck-btn--sm"
                    aria-label={`Remove task ${num}`}
                    onClick={() => removeTask(t.id)}
                  >
                    ×
                  </button>
                </span>
              </div>

              <div className="ck-night-line">
                <label className="ck-night-inline">
                  Agent
                  <select
                    className="ck-night-select"
                    aria-label={`Agent for task ${num}`}
                    value={ticked.includes(t.who) ? t.who : 'any'}
                    onChange={(e) => patch(t.id, { who: e.target.value })}
                  >
                    <option value="any">Any with usage left</option>
                    {ticked.map((key) => (
                      <option key={key} value={key}>
                        {labelOf(key)}
                      </option>
                    ))}
                  </select>
                </label>
                <span className="ck-night-inline">
                  Tests
                  <button
                    type="button"
                    className="ck-night-chip"
                    aria-label={`Tests for task ${num}`}
                    aria-haspopup="dialog"
                    aria-expanded={testsFor === t.id}
                    disabled={t.project === ''}
                    onClick={() => setTestsFor(testsFor === t.id ? null : t.id)}
                  >
                    {testsText(t)}
                  </button>
                </span>
                <button
                  type="button"
                  className="ck-btn ck-btn--plain ck-btn--sm"
                  aria-expanded={t.more}
                  onClick={() => patch(t.id, { more: !t.more })}
                >
                  More {t.more ? '▾' : '▸'}
                </button>
              </div>

              {testsFor === t.id && t.project !== '' && (
                <TestsPopover
                  project={t.project}
                  view={viewOf(t.project)}
                  onView={(view) => setChecks((cur) => ({ ...cur, [t.project]: view }))}
                  onNoTests={() => {
                    patch(t.id, { noTests: true });
                    setTestsFor(null);
                  }}
                  onClose={() => setTestsFor(null)}
                />
              )}

              {note && <p className="ck-night-note">{note}</p>}

              {t.more && (
                <div className="ck-night-more">
                  <span className="ck-night-inline">
                    <Switch
                      on={t.mayChangeTests}
                      label={`Agents may change the tests for task ${num}`}
                      onChange={(on) => patch(t.id, { mayChangeTests: on })}
                    />
                    Agents may change the tests
                  </span>
                  <label className="ck-night-inline">
                    Turns
                    <input
                      type="number"
                      className="ck-night-num"
                      min={MIN_TURNS}
                      max={MAX_TURNS}
                      value={t.turns}
                      onChange={(e) => patch(t.id, { turns: Number(e.target.value) || 0 })}
                    />
                  </label>
                  <label className="ck-night-inline">
                    Minutes
                    <input
                      type="number"
                      className="ck-night-num"
                      min={MIN_MINUTES}
                      max={MAX_MINUTES}
                      value={t.minutes}
                      onChange={(e) => patch(t.id, { minutes: Number(e.target.value) || 0 })}
                    />
                  </label>
                </div>
              )}
            </li>
          );
        })}
      </ul>

      <button
        type="button"
        className="ck-btn ck-btn--secondary ck-btn--sm ck-night-add"
        disabled={!state || tasks.length >= MAX_TASKS}
        onClick={addTask}
      >
        {`+ Add a task (${tasks.length} of ${MAX_TASKS})`}
      </button>

      <PlansPicker plans={plans ?? []} ticked={ticked} notice={notice} onToggle={togglePlan} />

      <div className="ck-night-line ck-night-when">
        <label className="ck-night-inline">
          Stop at
          <input
            type="time"
            className="ck-night-time"
            aria-label="Stop at"
            value={stopHhmm}
            onChange={(e) => setStopHhmm(e.target.value)}
          />
        </label>
        {stopWord && <span className="ck-night-hint">{stopWord}</span>}
        <span className="ck-night-inline" role="group" aria-label="Start">
          Start
          <label className="ck-night-inline">
            <input type="radio" name="ck-night-start" checked={startNow} onChange={() => setStartNow(true)} />
            Now
          </label>
          <label className="ck-night-inline">
            <input type="radio" name="ck-night-start" checked={!startNow} onChange={() => setStartNow(false)} />
            At
          </label>
          <input
            type="time"
            className="ck-night-time"
            aria-label="Start at"
            value={startHhmm}
            disabled={startNow}
            onChange={(e) => setStartHhmm(e.target.value)}
          />
        </span>
        <label className="ck-night-inline">
          At once
          <select
            className="ck-night-select"
            aria-label="At once"
            value={parallel}
            onChange={(e) => setParallel(Number(e.target.value))}
          >
            {Array.from({ length: MAX_PARALLEL }, (_, i) => i + 1).map((count) => (
              <option key={count} value={count}>
                {count}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="ck-night-line ck-night-awake">
        <span className="ck-night-inline">Keep this computer awake until it is done</span>
        <Switch
          on={keepAwake}
          label="Keep this computer awake until it is done"
          disabled={!loaded}
          onChange={awakeToggle}
        />
      </div>
      {confirmAwake && (
        <div className="ck-night-confirm" role="group" aria-label="Keep the computer awake">
          <p className="ck-night-note">{AWAKE_WORDS}</p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void answerAwake(false)}>
              Don&apos;t
            </button>
            <button type="button" className="ck-btn ck-btn--primary" onClick={() => void answerAwake(true)}>
              Keep it awake
            </button>
          </div>
        </div>
      )}

      <div className="ck-night-group-head">
        <span className="ck-night-label">Checks before you go</span>
      </div>
      <ul className="ck-night-checks-list">
        {undoOn ? (
          <li className="ck-night-ok">✓ Undo is on</li>
        ) : (
          <li className="ck-night-warn">! Undo is off</li>
        )}
        {gitOk ? (
          <li className="ck-night-ok">✓ Every project is a git project with a commit</li>
        ) : (
          <li className="ck-night-warn">! A project is not a git project with a commit</li>
        )}
        {untested > 0 ? (
          <li className="ck-night-warn">
            {`! ${untested} ${plural(untested, 'task has', 'tasks have')} no tests`}
          </li>
        ) : (
          n > 0 && <li className="ck-night-ok">✓ Every task has tests</li>
        )}
      </ul>
      {worst && <p className="ck-night-worst">{worst}</p>}

      {error && (
        <p className="ck-night-error" role="alert">
          {error}
        </p>
      )}
      {shownBlock && <p className="ck-night-blocked">{shownBlock}</p>}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="ck-btn ck-btn--primary"
          disabled={disabled}
          title={shownBlock ?? undefined}
          onClick={() => void start()}
        >
          {busy ? 'Starting…' : 'Start Night Shift'}
        </button>
      </div>
    </Sheet>
  );
}
