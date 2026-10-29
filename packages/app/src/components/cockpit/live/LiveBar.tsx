// The LIVE bar at the top of the cockpit while a Live Room runs. It has no close button: the room stops only
// by Stop, time up, idle, closed panes, a relay that ends it, or sleep. Inverted: near-black, white text.
import { useEffect, useState } from 'react';
import { liveRoomApi, minutesLeft, type LiveRoomApi, type LiveStatus } from '../../../lib/live-room';
import { copyRoomLink, errorText, useLiveRoom, type LiveSubscribe } from './useLiveRoom';
import '../../../styles/cockpit-live.css';

export function LiveBar({
  api = liveRoomApi,
  subscribe,
  onStopped,
}: {
  api?: LiveRoomApi;
  subscribe?: LiveSubscribe;
  /** Called after Stop works, so the app can show its toast. */
  onStopped?: () => void;
}) {
  const { status, refresh } = useLiveRoom({ api, subscribe });
  // Only a heartbeat to redraw: the minutes are read from the clock at render time. A time kept in state would be
  // as old as the bar (it is mounted long before a room starts) and show one minute too many.
  const [, setBeat] = useState(0);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The minutes left count down while the bar is up. A redraw every 15 s keeps it close enough.
  useEffect(() => {
    const timer = setInterval(() => setBeat((n) => n + 1), 15_000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  if (!status?.live) return null;
  const paused = status.paused;

  /** Runs one room action, then reads the status again. */
  const act = async (what: () => Promise<unknown>, after?: () => void) => {
    setBusy(true);
    setError(null);
    try {
      await what();
      after?.();
      refresh();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const copy = () => {
    setError(null);
    copyRoomLink(api)
      .then(() => setCopied(true))
      .catch((err) => setError(errorText(err)));
  };

  return (
    <div className={`ck-live-bar${paused ? ' is-paused' : ''}`} role="region" aria-label="Live Room">
      <span className="ck-live-tag">
        <span className={`ck-live-dot${paused ? ' is-still' : ''}`} aria-hidden="true" />
        {paused ? 'PAUSED' : 'LIVE'}
      </span>
      {paused ? (
        <span className="ck-live-text">viewers see &quot;Paused&quot;</span>
      ) : (
        <>
          <span className="ck-live-text">Live Room</span>
          <span className="ck-live-text">{watchingText(status)}</span>
          <span className="ck-live-text">{`${minutesLeft(status.endsAtMs, Date.now())} min left`}</span>
        </>
      )}
      {error && (
        <span className="ck-live-error" role="alert">
          {error}
        </span>
      )}
      <span className="ck-live-actions">
        {paused ? (
          <button type="button" className="ck-live-btn" disabled={busy} onClick={() => void act(() => api.resume())}>
            Resume
          </button>
        ) : (
          <>
            <button type="button" className="ck-live-btn" disabled={busy} onClick={() => void act(() => api.pause())}>
              Pause
            </button>
            <button type="button" className="ck-live-btn" disabled={busy} onClick={copy}>
              {copied ? 'Copied' : 'Copy link'}
            </button>
          </>
        )}
        <button type="button" className="ck-live-btn is-stop" disabled={busy} onClick={() => void act(() => api.stop(), onStopped)}>
          Stop
        </button>
      </span>
    </div>
  );
}

/** "1 watching", "7 watching". Viewers are counted by the relay; the owner sees only the number. */
export function watchingText(status: Pick<LiveStatus, 'viewers'>): string {
  return `${status.viewers} watching`;
}
