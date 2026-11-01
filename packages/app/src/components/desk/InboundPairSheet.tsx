// The question on the other screen: "<Name> wants to pair. Code 4F7Q-9K2M. Allow / No". Nothing is trusted until
// both computers' people say yes. Only a person at this window can answer: no agent, MCP tool or phone has a route.
// The focus starts on No, and Escape means No.
import { useId } from 'react';
import type { PairPending } from '../../lib/desk-pair';
import { Sheet } from '../cockpit/Sheet';

export function InboundPairSheet({
  pending,
  onAnswer,
}: {
  pending: PairPending | null;
  onAnswer: (id: number, allow: boolean) => void;
}) {
  const titleId = useId();
  if (!pending) return null;
  const asked = pending.role === 'host';
  return (
    <Sheet labelledBy={titleId} onClose={() => onAnswer(pending.id, false)}>
      <h2 className="ck-section" id={titleId}>
        {asked ? `${pending.name} wants to pair` : `Pair with ${pending.name}?`}
      </h2>
      <p className="ck-desk-code" aria-label={`Code ${pending.code}`}>
        {pending.code}
      </p>
      <p className="ck-sub">
        {asked
          ? 'Check that this is the code on your screen. Only say yes if you started this.'
          : 'Check that this is the code on the other computer.'}{' '}
        Nothing is shared yet. You choose what each computer may do afterwards.
      </p>
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn" autoFocus onClick={() => onAnswer(pending.id, false)}>
          No
        </button>
        <button type="button" className="ck-btn ck-btn--primary" onClick={() => onAnswer(pending.id, true)}>
          Allow
        </button>
      </div>
    </Sheet>
  );
}
