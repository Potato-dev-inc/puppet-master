// The LIVE chip in a shared pane's header. It shows only for panes in the room. Its menu holds the room's
// Pause (or Resume) and Stop, so a pane can be stopped from where the person is looking.
import { useEffect, useRef, useState } from 'react';
import { liveRoomApi, type LiveRoomApi, type LiveStatus } from '../../../lib/live-room';
import { errorText, useLiveRoom, type LiveSubscribe } from './useLiveRoom';
import '../../../styles/cockpit-live.css';

export function LiveChip({
  paneId,
  status: given,
  api = liveRoomApi,
  subscribe,
  onStopped,
}: {
  paneId: string;
  /** Pass the status when the parent already has it; otherwise the chip reads it itself. */
  status?: LiveStatus | null;
  api?: LiveRoomApi;
  subscribe?: LiveSubscribe;
  onStopped?: () => void;
}) {
  const own = useLiveRoom({ api, subscribe, enabled: given === undefined });
  const status = given === undefined ? own.status : given;
  const refresh = own.refresh;
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const wrap = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  if (!status?.live || !status.paneIds.includes(paneId)) return null;

  const run = (what: () => Promise<unknown>, after?: () => void) => {
    setError(null);
    setOpen(false);
    what()
      .then(() => {
        after?.();
        refresh();
      })
      .catch((err) => setError(errorText(err)));
  };

  return (
    <span className="ck-live-chip-wrap" ref={wrap}>
      <button
        type="button"
        className="ck-live-chip"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="ck-live-dot" aria-hidden="true" />
        LIVE
      </button>
      {open && (
        <div className="ck-live-chip-menu" role="menu" aria-label="Live Room">
          {status.paused ? (
            <button type="button" role="menuitem" className="ck-live-menu-item" onClick={() => run(() => api.resume())}>
              Resume
            </button>
          ) : (
            <button type="button" role="menuitem" className="ck-live-menu-item" onClick={() => run(() => api.pause())}>
              Pause
            </button>
          )}
          <button type="button" role="menuitem" className="ck-live-menu-item is-stop" onClick={() => run(() => api.stop(), onStopped)}>
            Stop
          </button>
        </div>
      )}
      {error && (
        <span className="ck-live-error" role="alert">
          {error}
        </span>
      )}
    </span>
  );
}
