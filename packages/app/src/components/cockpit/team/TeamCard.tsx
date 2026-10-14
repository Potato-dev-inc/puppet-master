import { useState } from 'react';
import { cardActions, cardMeta, cardTitle, type TeamApi, type TeamCard as Card } from '../../../lib/team';
import { DotSpinner } from '../DotSpinner';
import { formatElapsed } from '../model';
import { MergeSheet } from './MergeSheet';
import { QueueSheet } from './QueueSheet';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/**
 * A teammate's push or PR, in Inbox › Team. Everything in `card.event` came from a teammate and is
 * untrusted: it is rendered as plain text only, and the note always in quotes. Buttons: the one
 * action that fits (Pull now, or Queue my agent), Dismiss, and on an open GitHub PR "Merge when green"
 * (a sheet explains it first; GitHub does the merge).
 */
export function TeamCard({ card, api, now, onDone }: { card: Card; api: TeamApi; now: number; onDone?: () => void }) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [queueOpen, setQueueOpen] = useState(false);
  const [mergeOpen, setMergeOpen] = useState(false);
  const actions = cardActions(card);
  const e = card.event;

  const run = async (what: () => Promise<string | void>) => {
    setBusy(true);
    setError(null);
    try {
      const out = await what();
      if (typeof out === 'string') setMessage(out);
      onDone?.();
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="ck-inbox-item ck-team-card" data-team-card={card.id}>
      <div className="ck-inbox-top">
        <span className="ck-row-glyph">
          <DotSpinner status="idle" />
        </span>
        <span className="ck-row-title">{cardTitle(e)}</span>
        <span className="ck-row-time">{formatElapsed(now - e.atMs)} ago</span>
      </div>
      <p className="ck-team-meta">{cardMeta(e)}</p>
      {e.note && <p className="ck-team-note">“{e.note}”</p>}
      {(message ?? actions.note) && <p className="ck-team-meta">{message ?? actions.note}</p>}
      {error && (
        <p className="ck-team-error" role="alert">
          {error}
        </p>
      )}
      <div className="ck-inbox-actions">
        {actions.primary === 'pull' && (
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void run(() => api.pull(card.id))}>
            Pull now
          </button>
        )}
        {actions.primary === 'queue' && (
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => setQueueOpen(true)}>
            Queue my agent…
          </button>
        )}
        {actions.mergeWhenGreen && (
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => setMergeOpen(true)}>
            Merge when green
          </button>
        )}
        <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => void run(() => api.dismissCard(card.id))}>
          Dismiss
        </button>
      </div>
      {mergeOpen && (
        <MergeSheet
          card={card}
          api={api}
          onClose={() => setMergeOpen(false)}
          onDone={(text) => {
            setMessage(text);
            setMergeOpen(false);
            onDone?.();
          }}
        />
      )}
      {queueOpen && (
        <QueueSheet
          card={card}
          onClose={() => setQueueOpen(false)}
          onQueue={async (paneId, when) => {
            await api.queue(card.id, paneId, when);
            setQueueOpen(false);
            onDone?.();
          }}
        />
      )}
    </li>
  );
}
