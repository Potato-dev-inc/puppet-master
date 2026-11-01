// "Instructions" on the Advanced tab: the extra text for this bot. Each save is a version; the history lists them
// (InstructionHistory) with Restore. The text reaches the brain when it starts, like the persona.
import { useEffect, useState } from 'react';
import { ADVANCED_COPY, INSTRUCTIONS_COUNT_FROM, INSTRUCTIONS_MAX, type InstructionVersion } from '../../../lib/potato-botsettings';
import { AdvancedSection } from './AdvancedSection';
import { InstructionHistory } from './InstructionHistory';

export interface AdvancedInstructionsProps {
  instructions: string;
  versions: InstructionVersion[];
  /** Resolves true when the save went through. */
  onSave: (text: string) => Promise<boolean>;
  onRestore: (at: number) => Promise<boolean>;
}

export function AdvancedInstructions({ instructions, versions, onSave, onRestore }: AdvancedInstructionsProps) {
  const [draft, setDraft] = useState(instructions);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  // The text in use changes only by a save or a restore: then the box shows it.
  useEffect(() => setDraft(instructions), [instructions]);

  const dirty = draft.trim() !== instructions.trim();
  const tooLong = draft.trim().length > INSTRUCTIONS_MAX;
  const run = async (call: () => Promise<boolean>, done: string) => {
    setBusy(true);
    const ok = await call();
    setBusy(false);
    setStatus(ok ? done : null);
  };

  return (
    <AdvancedSection title={ADVANCED_COPY.instructions} line={ADVANCED_COPY.instructionsLine} id="ck-adv-instructions">
      <textarea
        className="ck-potato-adv-input ck-potato-adv-text"
        rows={6}
        value={draft}
        placeholder={ADVANCED_COPY.instructionsPlaceholder}
        aria-labelledby="ck-adv-instructions"
        onChange={(event) => {
          setDraft(event.target.value);
          setStatus(null);
        }}
      />
      <div className="ck-potato-adv-row">
        <span className={`ck-potato-adv-count${tooLong ? ' is-over' : ''}`} aria-live="polite">
          {draft.length >= INSTRUCTIONS_COUNT_FROM ? `${draft.length} / ${INSTRUCTIONS_MAX}` : ''}
        </span>
        <div className="ck-potato-adv-actions">
          <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" aria-expanded={historyOpen} onClick={() => setHistoryOpen((open) => !open)}>
            {ADVANCED_COPY.history}
          </button>
          <button
            type="button"
            className="ck-btn ck-btn--primary ck-btn--sm"
            disabled={!dirty || tooLong || busy}
            onClick={() => void run(() => onSave(draft), ADVANCED_COPY.saved)}
          >
            {ADVANCED_COPY.saveInstructions}
          </button>
        </div>
      </div>
      {status && (
        <p className="ck-potato-adv-status" role="status">
          {status}
        </p>
      )}
      {historyOpen && <InstructionHistory versions={versions} onRestore={(at) => run(() => onRestore(at), ADVANCED_COPY.restored)} />}
    </AdvancedSection>
  );
}
