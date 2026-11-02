// One line about the mouse between computers, and the two things the person may do about it: pause, or bring the
// mouse back. The line is told in words (never by colour alone). Nothing is shown while nothing is shared.
import { linkStatusText, type LinkView } from '../../lib/desk-link';

export function LinkStatus({
  view,
  error,
  onPause,
  onPanic,
}: {
  view: LinkView | null;
  error: string | null;
  onPause: (paused: boolean) => void;
  onPanic: () => void;
}) {
  const text = linkStatusText(view);
  if (!view || !view.supported) return null;
  return (
    <>
      {text && (
        <p className={`ck-desk-status${view.error ? ' is-error' : ''}`} role={view.error ? 'alert' : 'status'} data-testid="link-status">
          <span>{text}</span>
          {/* Being used from the other computer: the same button makes that computer let go and come home. */}
          {(view.mouse_on || view.controlled_by) && (
            <button type="button" className="ck-desk-btn" onClick={onPanic}>
              Bring the mouse back
            </button>
          )}
          {view.sharing && !view.mouse_on && (
            <button type="button" className="ck-desk-btn" onClick={() => onPause(!view.paused)}>
              {view.paused ? 'Resume sharing' : 'Pause sharing'}
            </button>
          )}
        </p>
      )}
      {view.clipboard_note && (
        <p className="ck-desk-hint" role="status">
          {view.clipboard_note}
        </p>
      )}
      {error && (
        <p className="ck-desk-hint is-error" role="alert">
          Could not do that: {error}
        </p>
      )}
    </>
  );
}
