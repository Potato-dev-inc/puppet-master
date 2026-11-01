// "Name" on the Advanced tab: the bot's name (the companion's name), saved on its own button. Blank names are refused by
// the engine; the button stays off until the name really changes.
import { useEffect, useState } from 'react';
import { ADVANCED_COPY } from '../../../lib/potato-botsettings';
import { AdvancedSection } from './AdvancedSection';

export interface AdvancedNameProps {
  name: string;
  /** Resolves true when the name was saved. */
  onSave: (name: string) => Promise<boolean>;
}

export function AdvancedName({ name, onSave }: AdvancedNameProps) {
  const [draft, setDraft] = useState(name);
  const [busy, setBusy] = useState(false);
  // A name that changes from outside (a save, a rename elsewhere) is shown at once.
  useEffect(() => setDraft(name), [name]);
  const clean = draft.trim();
  const changed = clean !== '' && clean !== name.trim();
  const save = async () => {
    setBusy(true);
    await onSave(clean);
    setBusy(false);
  };
  return (
    <AdvancedSection title={ADVANCED_COPY.name}>
      <form
        className="ck-potato-adv-row"
        onSubmit={(event) => {
          event.preventDefault();
          if (changed && !busy) void save();
        }}
      >
        <input
          className="ck-potato-adv-input"
          type="text"
          value={draft}
          maxLength={40}
          onChange={(event) => setDraft(event.target.value)}
          aria-label={ADVANCED_COPY.nameLabel}
        />
        <button type="submit" className="ck-btn ck-btn--secondary ck-btn--sm" disabled={!changed || busy}>
          {ADVANCED_COPY.saveName}
        </button>
      </form>
    </AdvancedSection>
  );
}
