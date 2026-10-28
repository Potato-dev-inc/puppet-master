// One task on the Morning page: a dense row with its result, and a details panel. Keep and Throw away take
// two presses: the first turns the buttons into a question, the second does it. The question goes back to
// the buttons after `CONFIRM_MS` when nothing is pressed. Every string here comes from the agent or from the
// person's own task, so it is shown as text only (React escapes it), never as HTML.
import { useEffect, useRef, useState } from 'react';
import type { NightAttemptEnd, NightAttemptView, NightCounts, NightTaskView } from '../../../lib/night';

/** How long a first press waits for the second one. */
export const CONFIRM_MS = 3_000;

type Step = 'keep' | 'throw';

export interface TaskRowProps {
  task: NightTaskView;
  /** A Keep or Throw away for this task (or for all of them) is on its way. */
  busy?: boolean;
  /** The last refusal for this task, in plain words. */
  error?: string | null;
  onKeep: (task: NightTaskView) => void;
  onThrowAway: (task: NightTaskView) => void;
  /** Opens the terminal of the task's last try. The button shows only when the task has a pane. */
  onOpenPane?: (paneId: string) => void;
}

/** "3 files", "1 turn": the count and the word, with an s when it is not one. */
export function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

/** The one-letter mark of a row: the UX 0.1 marks (~ + v : x). */
export function glyphOf(task: NightTaskView): string {
  if (task.review === 'discarded') return 'x';
  if (task.review === 'kept') return 'v';
  if (task.review === 'keep_failed') return '+';
  if (task.state === 'working') return '~';
  if (task.state === 'queued' || task.state === 'waiting') return ':';
  if (task.class === 'ready' || task.class === 'no_checks' || task.class === 'no_changes') return 'v';
  if (task.class === 'didnt_run') return 'x';
  return '+';
}

/** The question the first press of Keep asks. A task that needs a look asks with its reason. */
export function keepQuestion(task: NightTaskView): string {
  if (task.class === 'needs_look') {
    const why = task.reason ?? task.headline ?? 'It needs a look';
    return `${why}${/[.!?]$/.test(why) ? '' : '.'} Keep anyway?`;
  }
  return task.into ? `Merge into ${task.into}?` : 'Merge this copy?';
}

const END_WORDS: Record<NightAttemptEnd, string> = {
  running: 'still running',
  completed: 'finished',
  out_of_usage: 'ran out of usage',
  gate: 'stopped at a question',
  asked: 'asked a question',
  needs_you: 'needs you',
  failed: 'failed',
  cut_off: 'cut off at the stop time',
  stopped_by_person: 'stopped',
};

/** One try, in a line: its words when it has them, else how it ended and when. */
export function tryText(a: NightAttemptView): string {
  const head = `Try ${a.n} · ${a.label} · `;
  if (a.sentence) return head + a.sentence;
  return `${head}${END_WORDS[a.end]} ${a.ended ?? a.started}`;
}

function countsText(c: NightCounts): string {
  return `${c.passed} of ${c.total}`;
}

function filesText(task: NightTaskView): string | null {
  if (!task.files) return null;
  return `+${task.files.added} -${task.files.removed} · ${plural(task.files.count, 'file')}`;
}

function checksText(task: NightTaskView): string | null {
  if (!task.tests) return null;
  if (task.testsBefore) return `Tests ${countsText(task.testsBefore)} before, ${countsText(task.tests)} after`;
  return `Tests ${countsText(task.tests)}`;
}

export function TaskRow({ task, busy = false, error = null, onKeep, onThrowAway, onOpenPane }: TaskRowProps) {
  const [step, setStep] = useState<Step | null>(null);
  const [open, setOpen] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  function press(next: Step) {
    window.clearTimeout(timer.current);
    if (step !== next) {
      setStep(next);
      timer.current = window.setTimeout(() => setStep(null), CONFIRM_MS);
      return;
    }
    setStep(null);
    if (next === 'keep') onKeep(task);
    else onThrowAway(task);
  }

  // A kept or thrown-away copy is only a line now.
  if (task.review === 'kept' || task.review === 'discarded') {
    return (
      <li className="ck-morning-task is-closed">
        <div className="ck-morning-line">
          <span className="ck-morning-glyph" aria-hidden="true">
            {glyphOf(task)}
          </span>
          <span className="ck-morning-title">{task.title}</span>
          <span className="ck-morning-closed">{task.reviewText}</span>
        </div>
      </li>
    );
  }

  // Keep and Throw away only once the task has finished. A conflict row can still be thrown away.
  const finished = task.state === 'finished';
  const showKeep = finished && task.review === 'pending' && task.canKeep;
  const showThrow = finished && (task.canThrowAway || task.review === 'keep_failed');
  const showReason = Boolean(task.reason) && (task.class === 'needs_look' || task.class === 'didnt_run');
  const size = [filesText(task), task.duration].filter(Boolean).join(' · ');
  const checks = checksText(task);
  const pane = task.paneId;

  return (
    <li className="ck-morning-task">
      <div className="ck-morning-line">
        <span className="ck-morning-glyph" aria-hidden="true">
          {glyphOf(task)}
        </span>
        <span className="ck-morning-title">{task.title}</span>
        {task.project && <span className="ck-morning-project">{task.project}</span>}
        {task.plan && <span className="ck-morning-plan">{task.plan}</span>}
        <span className="ck-morning-headline">{task.headline ?? task.stateText}</span>
        {size && <span className="ck-morning-size">{size}</span>}
        <span className="ck-morning-actions">
          {step === 'keep' && (
            <>
              <span className="ck-morning-ask" role="status">
                {keepQuestion(task)}
              </span>
              <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => press('keep')}>
                Keep
              </button>
            </>
          )}
          {step === 'throw' && (
            <>
              <span className="ck-morning-ask" role="status">
                Delete this copy?
              </span>
              <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => press('throw')}>
                Throw away
              </button>
            </>
          )}
          {step === null && showKeep && (
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => press('keep')}>
              Keep
            </button>
          )}
          {step === null && showThrow && (
            <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => press('throw')}>
              Throw away
            </button>
          )}
        </span>
        <button
          type="button"
          className="ck-icon-btn ck-morning-toggle"
          aria-expanded={open}
          aria-label={open ? 'Hide details' : 'Show details'}
          onClick={() => setOpen((now) => !now)}
        >
          {open ? '^' : 'v'}
        </button>
      </div>

      {showReason && <p className="ck-morning-note">{task.reason}</p>}
      {task.review === 'keep_failed' && task.reviewText && (
        <p className="ck-morning-note" role="status">
          {task.reviewText}
        </p>
      )}
      {error && (
        <p className="ck-morning-note ck-morning-error" role="alert">
          {error}
        </p>
      )}

      {open && (
        <div className="ck-morning-details">
          {task.summaries.length > 0 && (
            <section>
              <p className="ck-morning-section">What it did</p>
              <ul className="ck-morning-list">
                {task.summaries.map((line, i) => (
                  <li key={i}>{line}</li>
                ))}
              </ul>
            </section>
          )}
          {filesText(task) && (
            <section>
              <p className="ck-morning-section">Changes</p>
              <p>{filesText(task)}</p>
            </section>
          )}
          {checks && (
            <section>
              <p className="ck-morning-section">Checks</p>
              <p>{checks}</p>
            </section>
          )}
          {task.testChangeLines.length > 0 && (
            <section>
              <p className="ck-morning-section">Test changes</p>
              <ul className="ck-morning-list">
                {task.testChangeLines.map((line, i) => (
                  <li key={i}>{line}</li>
                ))}
              </ul>
            </section>
          )}
          {task.attempts.length > 0 && (
            <section>
              <p className="ck-morning-section">Tries</p>
              <ul className="ck-morning-list">
                {task.attempts.map((a) => (
                  <li key={a.n}>{tryText(a)}</li>
                ))}
              </ul>
            </section>
          )}
          {pane && onOpenPane && (
            <button type="button" className="ck-btn ck-btn--plain" onClick={() => onOpenPane(pane)}>
              Open its terminal
            </button>
          )}
        </div>
      )}
    </li>
  );
}
