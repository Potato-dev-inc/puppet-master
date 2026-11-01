// "Add a computer": three ways in. Show a code (this computer is the one being found), Enter a code (this computer
// types what the other shows), Found nearby (pick the other from the list, then type its code). Progress and errors
// stay inside the sheet next to the button; an error is cleared only when the person types, never by a refresh.
import { useEffect, useId, useState } from 'react';
import type { PairHosting, PairNearby } from '../../lib/desk-pair';
import { Sheet } from '../cockpit/Sheet';

type Way = 'show' | 'enter' | 'nearby';

const WAYS: { id: Way; label: string }[] = [
  { id: 'show', label: 'Show a code' },
  { id: 'enter', label: 'Enter a code' },
  { id: 'nearby', label: 'Found nearby' },
];

/** "1:45" for the time left on a code. */
export function timeLeft(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

/** "14:02" */
function clock(ms: number): string {
  const d = new Date(ms);
  return `${d.getHours()}:${String(d.getMinutes()).padStart(2, '0')}`;
}

export function AddComputerSheet({
  hosting,
  nearby,
  joining,
  lockedUntil,
  onHostStart,
  onHostCancel,
  onJoin,
  onRefreshNearby,
  onClose,
  now = Date.now,
}: {
  hosting: PairHosting | null;
  nearby: PairNearby[];
  /** Progress words from the app while pairing. */
  joining: string | null;
  lockedUntil: number | null;
  /** Each resolves with the error in plain words, or null when it worked. */
  onHostStart: () => Promise<string | null>;
  onHostCancel: () => void;
  onJoin: (target: string, code: string) => Promise<string | null>;
  onRefreshNearby: () => void;
  onClose: () => void;
  now?: () => number;
}) {
  const titleId = useId();
  const [way, setWay] = useState<Way>('show');
  const [target, setTarget] = useState('');
  const [picked, setPicked] = useState<PairNearby | null>(null);
  const [code, setCode] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tick, setTick] = useState(() => now());

  useEffect(() => {
    const t = setInterval(() => setTick(now()), 1000);
    return () => clearInterval(t);
  }, [now]);
  useEffect(() => {
    if (way === 'nearby') onRefreshNearby();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [way]);

  const locked = lockedUntil !== null && lockedUntil > tick;
  const left = hosting ? hosting.expires_at - tick : 0;
  const showing = hosting !== null && left > 0;
  const close = () => {
    if (hosting) onHostCancel();
    onClose();
  };

  const pick = (next: Way) => {
    if (next !== 'show' && hosting) onHostCancel();
    setWay(next);
    setPicked(null);
    setError(null);
  };

  const showCode = async () => {
    setBusy(true);
    setError(await onHostStart());
    setBusy(false);
  };

  const pair = async () => {
    const to = picked ? picked.node : target.trim();
    if (!to) return setError('Paste the address from the other computer.');
    if (code.replace(/[\s-]/g, '').length !== 8) return setError('Type the 8-letter code from the other screen.');
    setBusy(true);
    setError(null);
    const failed = await onJoin(to, code);
    setBusy(false);
    if (failed) setError(failed);
  };

  return (
    <Sheet labelledBy={titleId} role="dialog" onClose={close} className="ck-desk-add">
      <h2 className="ck-section" id={titleId}>
        Add a computer
      </h2>
      <div className="ck-desk-tabs" role="tablist" aria-label="How to add it">
        {WAYS.map((w) => (
          <button
            key={w.id}
            type="button"
            role="tab"
            aria-selected={way === w.id}
            className={`ck-desk-tab${way === w.id ? ' is-on' : ''}`}
            onClick={() => pick(w.id)}
          >
            {w.label}
          </button>
        ))}
      </div>

      {locked && (
        <p className="ck-desk-hint is-error" role="alert">
          Pairing is paused until {clock(lockedUntil as number)}. There were too many wrong codes or answers.
        </p>
      )}

      {way === 'show' && (
        <div className="ck-desk-pane" role="tabpanel">
          {showing ? (
            <>
              <p className="ck-desk-code" aria-label={`Code ${hosting.code}`}>
                {hosting.code}
              </p>
              <p className="ck-sub" role="status">
                On the other computer, open My computers, press Add a computer and choose Enter a code. The code works
                once and ends in {timeLeft(left)}.
              </p>
              <label className="ck-desk-field">
                <span className="ck-desk-row-label">Address, if it does not show up nearby</span>
                <input className="ck-desk-input" readOnly value={hosting.ticket} onFocus={(e) => e.currentTarget.select()} />
              </label>
              <p className="ck-sub">Waiting… Keep PM open on both computers.</p>
            </>
          ) : (
            <p className="ck-sub">
              {hosting ? 'That code ended. ' : ''}Show a code on this computer, then type it on the other one.
            </p>
          )}
          {!showing && (
            <div className="ck-sheet-actions">
              <button type="button" className="ck-btn ck-btn--primary" disabled={busy || locked} onClick={() => void showCode()}>
                {hosting ? 'Show a new code' : 'Show a code'}
              </button>
            </div>
          )}
        </div>
      )}

      {way !== 'show' && (
        <div className="ck-desk-pane" role="tabpanel">
          {way === 'nearby' && !picked && (
            <>
              {nearby.length === 0 ? (
                <p className="ck-sub">
                  No computer is showing a code nearby. On the other computer, choose Add a computer › Show a code, or
                  use Enter a code with its address.
                </p>
              ) : (
                <ul className="ck-desk-nearby" aria-label="Computers nearby">
                  {nearby.map((n) => (
                    <li key={n.node}>
                      <button type="button" className="ck-desk-btn" onClick={() => setPicked(n)}>
                        {n.name}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <div className="ck-sheet-actions">
                <button type="button" className="ck-btn" onClick={onRefreshNearby}>
                  Look again
                </button>
              </div>
            </>
          )}

          {(way === 'enter' || picked) && (
            <>
              {picked ? (
                <p className="ck-sub">
                  Pair with <strong>{picked.name}</strong>. Type the code it shows.
                </p>
              ) : (
                <label className="ck-desk-field">
                  <span className="ck-desk-row-label">Address from the other computer</span>
                  <input
                    className="ck-desk-input"
                    value={target}
                    autoComplete="off"
                    spellCheck={false}
                    onChange={(e) => {
                      setTarget(e.target.value);
                      setError(null);
                    }}
                  />
                </label>
              )}
              <label className="ck-desk-field">
                <span className="ck-desk-row-label">Code</span>
                <input
                  className="ck-desk-input ck-desk-code-input"
                  value={code}
                  maxLength={10}
                  autoComplete="off"
                  spellCheck={false}
                  placeholder="4F7Q-9K2M"
                  onChange={(e) => {
                    setCode(e.target.value.toUpperCase());
                    setError(null);
                  }}
                />
              </label>
              <div className="ck-desk-progress">
                <button type="button" className="ck-btn ck-btn--primary" disabled={busy || locked} onClick={() => void pair()}>
                  Pair
                </button>
                {busy && (
                  <span className="ck-sub" role="status">
                    {joining ?? 'Looking for the other computer… Keep PM open on both computers.'}
                  </span>
                )}
              </div>
            </>
          )}
        </div>
      )}

      {error && (
        <p className="ck-desk-hint is-error" role="alert">
          {error}
        </p>
      )}

      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn" onClick={close}>
          {hosting ? 'Cancel' : 'Close'}
        </button>
      </div>
    </Sheet>
  );
}
