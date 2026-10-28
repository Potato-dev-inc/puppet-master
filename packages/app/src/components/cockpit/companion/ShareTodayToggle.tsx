// "Let {name} see what my agents did today" (Setup card). Off by default. When it is on, Puppet Master writes a short
// list of the other agents' finished turns into the companion's own folder (today.md), with secrets hidden. The switch
// only sends one `update` call; the backend writes the file and tells a running companion once.
import { useState } from 'react';
import type { CompanionTransport, CompanionView } from '../../../lib/companion';

export function ShareTodayToggle({
  companionId,
  name,
  shareToday,
  transport,
  onChanged,
}: {
  companionId: string;
  name: string;
  /** `undefined` (a view from before the setting) counts as off. */
  shareToday?: boolean;
  transport: CompanionTransport;
  onChanged: (next: CompanionView) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const on = shareToday ?? false;
  const label = `Let ${name} see what my agents did today`;

  const toggle = async () => {
    if (busy) return;
    setError(null);
    setBusy(true);
    try {
      onChanged(await transport.update(companionId, { shareToday: !on }));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not save the setting.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ck-companion-share">
      <div className="ck-companion-push">
        <span className="ck-companion-push-text">
          <span>{label}</span>
          <span className="ck-sub">A short list of what they finished, kept in its folder.</span>
        </span>
        <button
          type="button"
          role="switch"
          aria-checked={on}
          aria-label={label}
          className="ck-switch"
          disabled={busy}
          onClick={() => void toggle()}
        >
          <span className="ck-switch-knob" />
        </button>
      </div>
      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
