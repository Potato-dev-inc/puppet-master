// The "Restore ...?" confirmation: what a restore does, the safety copy, and (when another agent
// works in the same folder) the shared-folder warning. Used by the History tab and by the palette's
// "Undo last turn", so every way to restore asks the same question.
import { sharedFolderWarning } from './history-model';

export function RestoreSheet({
  title,
  confirmLabel = 'Restore files',
  sharedWith = [],
  busy,
  onCancel,
  onConfirm,
}: {
  title: string;
  confirmLabel?: string;
  /** Labels of the other live panes working in this pane's folder. */
  sharedWith?: string[];
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const warning = sharedFolderWarning(sharedWith);
  return (
    <div className="ck-scrim" onMouseDown={() => !busy && onCancel()}>
      <div
        className="ck-sheet"
        role="alertdialog"
        aria-labelledby="ck-restore-t"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2 className="ck-section" id="ck-restore-t">
          {title}
        </h2>
        <p className="ck-sub">
          This restores your files. It doesn't change what the agent remembers. Files created since then are removed,
          and files in .gitignore are left alone.
        </p>
        <p className="ck-sub">Your current files are saved first, so you can undo this from History.</p>
        {warning && <p className="ck-sub">{warning}</p>}
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={onConfirm}>
            {busy ? 'Restoring…' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
