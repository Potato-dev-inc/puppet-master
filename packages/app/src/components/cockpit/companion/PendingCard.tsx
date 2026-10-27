// "{name} wants to remember" (SPEC section 6): the notes the companion proposed, each kept or dropped by the person.
// Flags never block: a link or an instruction-like line shows a small tag and still waits for the person.
import { useState } from 'react';
import type { CompanionTransport, MemoryView, PendingDecision, PendingItem } from '../../../lib/companion';

export function PendingCard({
  companionId,
  name,
  items,
  transport,
  onChanged,
}: {
  companionId: string;
  name: string;
  items: PendingItem[];
  transport: CompanionTransport;
  onChanged: (next: MemoryView) => void;
}) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (items.length === 0) return null;

  const decide = async (item: PendingItem, decision: PendingDecision) => {
    setBusyId(item.id);
    setError(null);
    try {
      onChanged(await transport.decidePending(companionId, item.id, decision));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not save that.');
    } finally {
      setBusyId(null);
    }
  };

  return (
    <section className="ck-companion-card" aria-labelledby="ck-companion-pending-title">
      <h2 className="ck-companion-card-title" id="ck-companion-pending-title">
        {name} wants to remember
      </h2>
      <ul className="ck-companion-list">
        {items.map((item) => (
          <li className="ck-companion-pending" key={item.id}>
            <p className="ck-companion-pending-text">{item.text}</p>
            {item.targetLine && <p className="ck-sub ck-companion-pending-target">Removes: {item.targetLine}</p>}
            {item.flagged && <span className="ck-memory-tag">{item.flagged}</span>}
            <span className="ck-companion-pending-actions">
              <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => void decide(item, 'keep')} disabled={busyId !== null}>
                Keep
              </button>
              <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" onClick={() => void decide(item, 'drop')} disabled={busyId !== null}>
                Don't keep
              </button>
            </span>
          </li>
        ))}
      </ul>
      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
