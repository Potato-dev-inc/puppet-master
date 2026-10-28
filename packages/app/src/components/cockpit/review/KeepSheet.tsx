// The confirm sheet for Keep. Merge (a copy or a Night Shift task) asks `Merge into main?`; an issue run asks
// for a draft pull request. Nothing happens until the primary button or Enter is pressed, and Enter confirms
// once. Refusals use the same plain sentences as the Copies tab (`plainMergeError`).
import { useRef, useState, type KeyboardEvent } from 'react';
import { plainMergeError } from '../Changes';
import { Sheet } from '../Sheet';
import { plural } from '../diff';
import { reviewDeskApi, reviewDeskError, type ReviewAct, type ReviewItem } from '../../../lib/review-desk';

/** The one call this sheet makes. A test double can stand in for it. */
export type ReviewKeepApi = Pick<typeof reviewDeskApi, 'keep'>;

export interface KeepSheetProps {
  item: ReviewItem;
  /** Issue runs: the push-check reasons (the detail's `extra`). */
  extra?: string[];
  api?: ReviewKeepApi;
  /** Keep worked. The parent closes the sheet and refreshes the list. */
  onDone: (act: ReviewAct) => void;
  onCancel: () => void;
}

const TITLE_ID = 'ck-rd-keep-title';

/** The copy of a Keep sheet for one kind. Merge and Night Shift share the same words. */
export function keepSheetCopy(item: ReviewItem) {
  if (item.keep.kind === 'issue_push') {
    return {
      title: 'Push as a draft pull request?',
      primary: 'Push as draft',
      footer: 'Puppet Master opens a draft. It never merges.',
    };
  }
  const into = item.keep.into || 'your project';
  const n = item.files.total;
  return {
    title: `Merge into ${into}?`,
    primary: 'Merge',
    body: `The copy's ${plural(n, 'file')} ${n === 1 ? 'goes' : 'go'} into ${into}. The copy is closed after the merge.`,
  };
}

export function KeepSheet({ item, extra = [], api = reviewDeskApi, onDone, onCancel }: KeepSheetProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ text: string; title?: string } | null>(null);
  // A ref, not state: a second Enter in the same tick must not call Keep twice.
  const busyRef = useRef(false);
  const copy = keepSheetCopy(item);
  const issue = item.keep.kind === 'issue_push';
  // Look first: the riskiest reason, only when the item is at tier 4 or higher (riskier than a test change).
  const warn = !issue && item.tier <= 4 && item.reasons[0] ? item.reasons[0].sentence : null;

  const confirm = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const act = await api.keep(item.key, item.fingerprint);
      onDone(act);
    } catch (err) {
      busyRef.current = false;
      setBusy(false);
      const failure = reviewDeskError(err);
      const mapped = plainMergeError(failure.code ? `${failure.code}: ${failure.message}` : failure.message);
      setError({ text: mapped.title ? mapped.text : failure.message, title: mapped.title });
    }
  };

  // Enter confirms. A focused button keeps its own Enter (a click), so the button is skipped here.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'Enter' || e.target instanceof HTMLButtonElement) return;
    e.preventDefault();
    void confirm();
  };

  return (
    <Sheet labelledBy={TITLE_ID} onClose={busy ? undefined : onCancel} className="ck-rd-sheet">
      <div className="ck-rd-sheet-body" onKeyDown={onKeyDown}>
        <h2 className="ck-section" id={TITLE_ID}>
          {copy.title}
        </h2>
        {issue ? (
          extra.length > 0 ? (
            <ul className="ck-rd-sheet-list">
              {extra.map((line, i) => (
                <li key={i}>{line}</li>
              ))}
            </ul>
          ) : (
            <p className="ck-sub">Puppet Master found no push warnings.</p>
          )
        ) : (
          <p className="ck-sub">{copy.body}</p>
        )}
        {warn && <p className="ck-rd-sheet-warn">{`Look first: ${warn}`}</p>}
        {error && (
          <p className="ck-rd-sheet-error" role="alert" title={error.title}>
            {error.text}
          </p>
        )}
        {issue && <p className="ck-rd-sheet-foot">{copy.footer}</p>}
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="ck-btn ck-btn--primary" autoFocus disabled={busy} onClick={() => void confirm()}>
            {copy.primary}
          </button>
        </div>
      </div>
    </Sheet>
  );
}
