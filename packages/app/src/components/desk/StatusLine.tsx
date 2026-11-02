// One plain line under the title: what screen hops are doing right now. Errors get one fix button.
import type { DeskStatus } from '../../lib/desk';

/** The words for a status. Pure, so the page and its tests read the same text. */
export function statusText(status: DeskStatus, screenCount: number): string {
  if (status.error) return `Screen hops hit a problem: ${status.error}`;
  if (status.state === 'unsupported') return "Screen hops don't work on this computer yet.";
  if (status.state === 'starting') return 'Starting…';
  if (!status.hops_enabled) return 'Screen hops are off';
  if (status.paused || status.state === 'paused') return 'Paused';
  const on = `On · ${screenCount} ${screenCount === 1 ? 'screen' : 'screens'}`;
  return status.pointer_locked ? `${on} · Pointer locked to this screen` : on;
}

export function StatusLine({
  status,
  screenCount,
  onFix,
}: {
  status: DeskStatus;
  screenCount: number;
  onFix: () => void;
}) {
  const hasError = Boolean(status.error);
  return (
    <p className={`ck-desk-status${hasError ? ' is-error' : ''}`} role={hasError ? 'alert' : 'status'}>
      <span>{statusText(status, screenCount)}</span>
      {hasError && (
        <button type="button" className="ck-desk-btn" onClick={onFix}>
          Try again
        </button>
      )}
    </p>
  );
}
