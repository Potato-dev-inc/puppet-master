// The Focus note at the top of the Inbox (SPEC n33 3.6 (1)). Off: "Focus" and three length chips, the default
// one marked. On: the time left, what Focus is holding, "+15 min" and "End Focus", and the finished
// missions it has folded away. Grayscale only: an urgent count is plain text, the card carries the weight.
import { useState } from 'react';
import { FOCUS_PRESETS, focusEnd, focusExtend, focusStart, heldText, leftText, type FocusView } from '../../lib/focus';
import '../../styles/focus-note.css';

export function FocusNote({
  view,
  now,
  foldedCount = 0,
  onShowFinished,
  onChange,
}: {
  /** The Focus view; null (not read yet, or no desktop shell) draws nothing. */
  view: FocusView | null;
  /** The clock for "min left". */
  now: number;
  /** Finished missions Focus is holding back from this list (0: none, or Focus is off). */
  foldedCount?: number;
  /** "Show": unfold the finished missions for this view. */
  onShowFinished?: () => void;
  /** A command's answer, so the note shows the new state at once. */
  onChange?: (view: FocusView) => void;
}) {
  const [failed, setFailed] = useState(false);
  if (!view) return null;

  const run = (call: Promise<FocusView>) => {
    setFailed(false);
    call.then((next) => onChange?.(next)).catch(() => setFailed(true));
  };
  const error = failed ? <span className="ck-focus-note-error">Couldn't change Focus. Try again.</span> : null;

  if (!view.on) {
    return (
      <div className="ck-focus-note" data-focus="off" role="group" aria-label="Focus">
        <span className="ck-focus-note-label">Focus</span>
        {FOCUS_PRESETS.map((m) => {
          const isDefault = m === view.settings.default_minutes;
          return (
            <button
              key={m}
              type="button"
              className={`ck-chip ck-focus-note-chip${isDefault ? ' is-default' : ''}`}
              data-default={isDefault ? 'true' : undefined}
              onClick={() => run(focusStart(m))}
            >
              {m} min
            </button>
          );
        })}
        {error}
      </div>
    );
  }

  const urgent = view.waitingUrgent;
  return (
    <div className="ck-focus-note" data-focus="on" role="group" aria-label="Focus">
      <div className="ck-focus-note-row">
        <span className="ck-focus-note-title">Focus · {leftText(view.endsAtMs ?? now, now)}</span>
        {urgent > 0 && <span className="ck-focus-note-urgent">{urgent} urgent</span>}
      </div>
      <p className="ck-focus-note-sub">{heldText(view)}. Things that need you still show.</p>
      <div className="ck-focus-note-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={() => run(focusExtend())}>
          +15 min
        </button>
        <button type="button" className="ck-btn ck-btn--plain" onClick={() => run(focusEnd())}>
          End Focus
        </button>
      </div>
      {foldedCount > 0 && (
        <p className="ck-focus-note-sub">
          {foldedCount} finished {foldedCount === 1 ? 'mission waits' : 'missions wait'} until Focus ends{' '}
          <button type="button" className="ck-focus-note-link" onClick={() => onShowFinished?.()}>
            Show
          </button>
        </p>
      )}
      {error}
    </div>
  );
}
