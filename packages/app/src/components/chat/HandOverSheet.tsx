// The question before a hand-over that would stop something: it shows the backend's own sentence and two buttons.
import { HANDOVER_WORDS } from '../../lib/acpChat';
import type { HandOverAsk } from '../../hooks/useHandOver';
import { Sheet } from '../cockpit/Sheet';

export function HandOverSheet({ ask, busy, onConfirm, onCancel }: { ask: HandOverAsk; busy: boolean; onConfirm: () => void; onCancel: () => void }) {
  const words = HANDOVER_WORDS[ask.kind];
  return (
    <Sheet labelledBy="ck-handover-t" onClose={busy ? undefined : onCancel}>
      <h2 className="ck-section" id="ck-handover-t">
        {words.title}
      </h2>
      <p className="ck-sub" data-testid="handover-ask">
        {ask.text}
      </p>
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={onCancel}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={onConfirm}>
          {words.yes}
        </button>
      </div>
    </Sheet>
  );
}
