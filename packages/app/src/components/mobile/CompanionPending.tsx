// "{name} wants to remember" on the companion home (SPEC-1028 section 6): the notes the companion proposed and the
// person has not decided yet. Keep / Don't keep need the `answer` cap; without it the notes are only read.
import type { PendingDecision, PendingItem } from '../../lib/companion';

export function CompanionPending({
  name,
  items,
  canAnswer,
  busyId,
  onDecide,
}: {
  name: string;
  items: PendingItem[];
  /** This phone may keep or drop notes (`answer`). */
  canAnswer: boolean;
  /** The item being sent right now: its buttons are off until the PC answers. */
  busyId: string | null;
  onDecide: (itemId: string, decision: PendingDecision) => void;
}) {
  if (items.length === 0) return null;
  return (
    <section className="ph-comp-block" aria-label={`${name} wants to remember`}>
      <h2 className="ph-comp-title">{name} wants to remember</h2>
      <ul className="ph-group">
        {items.map((item) => (
          <li key={item.id} className="ph-comp-pending">
            {item.kind === 'skill' && <p className="ph-comp-skill-kind">New skill</p>}
            <p className="ph-comp-text">{item.text}</p>
            {item.flagged && <p className="ph-comp-flag">{item.flagged}</p>}
            {/* A skill is approved on the computer, where all of its steps can be read. */}
            {item.kind === 'skill' && <p className="ph-comp-flag">Look at it on the computer to approve it.</p>}
            {canAnswer && item.kind !== 'skill' && (
              <div className="ph-comp-pending-btns">
                <button
                  type="button"
                  className="ph-btn ph-btn--primary"
                  disabled={busyId !== null}
                  onClick={() => onDecide(item.id, 'keep')}
                >
                  Keep
                </button>
                <button type="button" className="ph-btn" disabled={busyId !== null} onClick={() => onDecide(item.id, 'drop')}>
                  Don't keep
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
