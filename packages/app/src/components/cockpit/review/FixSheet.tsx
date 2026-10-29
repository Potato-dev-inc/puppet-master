// The Ask to fix sheet: shows the fixed lines and an optional note, then puts them in the agent's chat box.
// Nothing is sent. The backend builds the real draft; this preview only shows what it will hold.
import { useRef, useState, type KeyboardEvent } from 'react';
import { Sheet } from '../Sheet';
import { reviewDeskApi, reviewDeskErrorText, type ReviewItem } from '../../../lib/review-desk';

/** The most file names on one line of the draft. */
export const FIX_NAMES_SHOWN = 3;
/** The person's note is cut at this length (the textarea enforces it too). */
export const FIX_NOTE_MAX = 1000;

/** The lines the draft holds: the reasons, the file names, then the person's note. */
export function fixPreviewLines(item: ReviewItem, note: string): string[] {
  const lines = item.reasons.map((reason) => `- ${reason.sentence}`);
  const rows = item.files.rows;
  if (rows.length > 0) {
    const shown = rows.slice(0, FIX_NAMES_SHOWN).map((row) => row.path);
    const more = item.files.total - shown.length;
    lines.push(`Files: ${shown.join(', ')}${more > 0 ? ` and ${more} more` : ''}`);
  }
  const trimmed = note.trim();
  if (trimmed) lines.push(`Note: ${trimmed}`);
  return lines;
}

/** The fix API slice (`review_desk_fix`). A test double can stand in for it. */
export type ReviewFixSheetApi = Pick<typeof reviewDeskApi, 'fix'>;

export interface FixSheetProps {
  item: ReviewItem;
  api?: ReviewFixSheetApi;
  /** The draft is in the chat box. The parent opens that pane. */
  onDone: (paneId: string) => void;
  onCancel: () => void;
}

const TITLE_ID = 'ck-rd-fix-title';

export function FixSheet({ item, api = reviewDeskApi, onDone, onCancel }: FixSheetProps) {
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A ref, not state: a second Enter in the same tick must not put the draft in twice.
  const busyRef = useRef(false);

  const send = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const paneId = await api.fix(item.key, item.fingerprint, note.trim());
      onDone(paneId);
    } catch (err) {
      busyRef.current = false;
      setBusy(false);
      setError(reviewDeskErrorText(err));
    }
  };

  // Enter puts the draft in the chat box; Shift+Enter adds a line to the note.
  const onNoteKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== 'Enter' || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    void send();
  };

  return (
    <Sheet labelledBy={TITLE_ID} onClose={busy ? undefined : onCancel} className="ck-rd-sheet">
      <h2 className="ck-section" id={TITLE_ID}>
        Ask to fix
      </h2>
      <p className="ck-sub">Puppet Master writes these lines in the agent&apos;s chat box.</p>
      <pre className="ck-rd-fix-preview" aria-label="What goes in the chat box">
        {fixPreviewLines(item, note).join('\n')}
      </pre>
      <label className="ck-rd-fix-note">
        <span className="ck-sub">Add a note (optional)</span>
        <textarea
          value={note}
          maxLength={FIX_NOTE_MAX}
          rows={3}
          autoFocus
          disabled={busy}
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={onNoteKeyDown}
        />
      </label>
      {error && (
        <p className="ck-rd-sheet-error" role="alert">
          {error}
        </p>
      )}
      <p className="ck-rd-sheet-foot">It is not sent. You can edit it in the chat box.</p>
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onCancel}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void send()}>
          Put in chat
        </button>
      </div>
    </Sheet>
  );
}
