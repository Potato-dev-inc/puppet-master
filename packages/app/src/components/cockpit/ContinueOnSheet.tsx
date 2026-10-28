// "Other agent…": the sheet a usage-limit card opens for a pane that is not a companion's. The person picks another
// agent; Puppet Master stops this pane's agent first, then starts the picked one in the same folder with a short
// brief (src/lib/keep-going.ts). Only the rows the server marks ok can be picked; the others say why not.
import { useEffect, useState } from 'react';
import { keepGoingContinue, keepGoingSettingsGet, keepGoingTargetKey, keepGoingTargets, type KeepGoingOffer, type KeepGoingSettings } from '../../lib/keep-going';
import '../../styles/cockpit-keep-going.css';
import { Sheet } from './Sheet';

const GENERIC_ERROR = 'Something went wrong. Try again.';

/** Backend errors read `CODE: message`. The person sees only the message, in plain words. */
function friendlyError(err: unknown): string {
  const raw = err instanceof Error ? err.message : typeof err === 'string' ? err : '';
  const text = raw.replace(/^[A-Z][A-Z0-9_]{2,}:\s*/, '').trim();
  if (!text || /^[A-Z][A-Z0-9_]+$/.test(text)) return GENERIC_ERROR;
  return text;
}

/** The clock time of a moment, as "17:05". */
function clockText(ms: number): string {
  const at = new Date(ms);
  return `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
}

export function ContinueOnSheet({
  paneId,
  cardId,
  fromLabel,
  resetAtMs,
  onClose,
}: {
  /** The pane whose agent ran out. */
  paneId: string;
  /** The usage-limit card; the backend dismisses it when the hand-over starts. */
  cardId: string;
  /** The agent that ran out, in the words the card uses ("Claude Code"). */
  fromLabel: string;
  /** When the limit ends (epoch ms), when the card knows. The move-back switch only shows then. */
  resetAtMs?: number;
  onClose: () => void;
}) {
  const [offers, setOffers] = useState<KeepGoingOffer[] | null>(null);
  const [settings, setSettings] = useState<KeepGoingSettings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    keepGoingTargets(paneId)
      .then((list) => {
        if (!live) return;
        setOffers(list);
        const first = list.find((o) => o.ok);
        setPicked(first ? keepGoingTargetKey(first.target) : null);
      })
      .catch((err) => live && setLoadError(friendlyError(err)));
    keepGoingSettingsGet()
      .then((saved) => live && setSettings(saved))
      .catch((err) => live && setLoadError(friendlyError(err)));
    return () => {
      live = false;
    };
  }, [paneId]);

  const chosen = (offers ?? []).find((o) => o.ok && keepGoingTargetKey(o.target) === picked) ?? null;
  // The engine refuses a `back` that differs from the saved setting, so the saved value is always what goes.
  const canContinue = chosen !== null && settings !== null && !busy;

  const go = async () => {
    if (!chosen || !settings) return;
    setBusy(true);
    setError(null);
    try {
      await keepGoingContinue(paneId, keepGoingTargetKey(chosen.target), settings.ask_back, cardId);
      onClose();
    } catch (err) {
      setError(friendlyError(err));
      setBusy(false);
    }
  };

  return (
    <Sheet onClose={busy ? undefined : onClose} labelledBy="ck-keep-going-title" className="ck-keep-going-sheet" role="dialog">
      <h3 className="ck-section" id="ck-keep-going-title">
        Continue on another agent
      </h3>
      {offers === null && !loadError && <p className="ck-sub">Looking at your agents…</p>}
      {offers !== null && offers.length === 0 && (
        <p className="ck-sub">No other agent can run in this folder yet. Pick one in Settings &gt; Agents &gt; Keep going.</p>
      )}
      {offers !== null && offers.length > 0 && (
        <div className="ck-keep-going-list" role="radiogroup" aria-label="Agent">
          {offers.map((o) => {
            const key = keepGoingTargetKey(o.target);
            return (
              <label key={key} className={`ck-keep-going-row${o.ok ? '' : ' is-off'}`}>
                <input
                  type="radio"
                  name="ck-keep-going-agent"
                  value={key}
                  checked={picked === key}
                  disabled={!o.ok || busy}
                  onChange={() => setPicked(key)}
                />
                <span className="ck-keep-going-name">{o.label}</span>
                <span className="ck-keep-going-state">{o.ok ? 'Ready' : (o.whyNot ?? 'Not available right now')}</span>
              </label>
            );
          })}
        </div>
      )}
      {resetAtMs !== undefined && settings && (
        <label className="ck-keep-going-back">
          <input type="checkbox" checked={settings.ask_back} disabled />
          <span>Ask me to move it back at {clockText(resetAtMs)}</span>
          <span className="ck-sub">Set in Settings &gt; Agents &gt; Keep going.</span>
        </label>
      )}
      <p className="ck-sub ck-keep-going-copy">
        The agent gets a short brief. {fromLabel} in this pane is stopped first. Puppet Master never answers its questions for you.
      </p>
      {loadError && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {loadError}
        </p>
      )}
      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--sm ck-btn--primary" disabled={!canContinue} onClick={() => void go()}>
          {busy ? 'Starting…' : chosen ? `Continue on ${chosen.label}` : 'Continue'}
        </button>
      </div>
    </Sheet>
  );
}
