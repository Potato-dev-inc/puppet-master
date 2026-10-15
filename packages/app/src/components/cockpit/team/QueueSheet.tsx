import { useState } from 'react';
import { whoDid, type TeamCard } from '../../../lib/team';
import { Sheet } from '../Sheet';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/**
 * "Bring feat/auth into feat/billing": which agent does it, and when. The work happens in a
 * separate copy of my branch; my files change only when I apply it.
 */
export function QueueSheet({
  card,
  onClose,
  onQueue,
}: {
  card: TeamCard;
  onClose: () => void;
  onQueue: (paneId: string, when: 'idle' | 'now') => Promise<void>;
}) {
  const panes = card.panes;
  const [paneId, setPaneId] = useState(panes[0]?.paneId ?? '');
  const pane = panes.find((p) => p.paneId === paneId);
  const busyPane = pane?.status === 'running' || pane?.status === 'waiting_input';
  const [when, setWhen] = useState<'idle' | 'now'>('idle');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const theirs = card.event.subject.branch ?? 'their branch';
  const mine = card.local?.currentBranch ?? 'your branch';
  const name = (p: (typeof panes)[number]) => (p.title ? `${p.label} · ${p.title}` : p.label);

  const submit = async () => {
    if (!paneId) return;
    setBusy(true);
    setError(null);
    try {
      await onQueue(paneId, busyPane ? when : 'now');
    } catch (err) {
      setError(errText(err));
      setBusy(false);
    }
  };

  return (
    <Sheet labelledBy="ck-team-queue-t" role="dialog" onClose={busy ? undefined : onClose} className="ck-team-sheet">
      <h2 className="ck-section" id="ck-team-queue-t">
        Bring {theirs} into {mine}
      </h2>
      <p className="ck-sub">
        From {whoDid(card.event)}. Your agent merges it in a separate copy of your branch. Your files don't change until you apply.
      </p>
      {panes.length === 0 ? (
        <p className="ck-team-meta">Open an agent in {card.event.project.name} first, then queue it.</p>
      ) : (
        <>
          <label className="ck-team-field">
            <span>Who</span>
            <select value={paneId} onChange={(e) => setPaneId(e.target.value)} disabled={busy}>
              {panes.map((p) => (
                <option key={p.paneId} value={p.paneId}>
                  {name(p)}
                </option>
              ))}
            </select>
          </label>
          {busyPane && (
            <fieldset className="ck-team-when">
              <legend>When</legend>
              <label>
                <input type="radio" name="when" checked={when === 'idle'} onChange={() => setWhen('idle')} disabled={busy} />
                After {pane?.label} finishes its current turn
              </label>
              <label>
                <input type="radio" name="when" checked={when === 'now'} onChange={() => setWhen('now')} disabled={busy} />
                Now, alongside it
              </label>
            </fieldset>
          )}
        </>
      )}
      {card.local?.dirty && (
        <p className="ck-team-meta">Your uncommitted changes aren't included. Commit or stash them first to include them.</p>
      )}
      <p className="ck-team-meta">If nothing conflicts, no agent is needed. Nothing is ever pushed.</p>
      {error && (
        <p className="ck-team-error" role="alert">
          {error}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onClose}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" disabled={busy || !paneId} onClick={() => void submit()}>
          Queue it
        </button>
      </div>
    </Sheet>
  );
}
