// One companion in the sidebar, above Inbox: its face, its name and its state word (SPEC section 8).
// With no companion yet, one quiet row asks the person to make one.
import type { CompanionView } from '../../../lib/companion';
import { stateWord } from '../../../lib/companion';
import { Avatar } from '../../companion/Avatar';
import '../../../styles/cockpit-companion.css';

export function CompanionRow({ companion, active, onOpen }: { companion: CompanionView; active: boolean; onOpen: () => void }) {
  const needs = companion.state === 'needs_you';
  return (
    <button
      type="button"
      className={`ck-row ck-companion-row${active ? ' is-active' : ''}${needs ? ' is-needs' : ''}`}
      onClick={onOpen}
      aria-current={active ? 'true' : undefined}
    >
      <span className="ck-row-glyph ck-companion-face">
        <Avatar id={companion.avatar} size={24} state={companion.state} name={companion.name} />
      </span>
      <span className="ck-row-main">
        <span className="ck-row-title">{companion.name}</span>
      </span>
      <span className="ck-companion-state">{stateWord(companion.state)}</span>
    </button>
  );
}

/** The quiet row shown while the person has no companion. */
export function MakeCompanionRow({ onMake }: { onMake: () => void }) {
  return (
    <button type="button" className="ck-row ck-companion-row ck-companion-row--empty" onClick={onMake}>
      <span className="ck-row-glyph ck-companion-face">
        <span className="ck-companion-outline" aria-hidden="true" />
      </span>
      <span className="ck-row-main">
        <span className="ck-row-title">Make a companion</span>
      </span>
    </button>
  );
}
