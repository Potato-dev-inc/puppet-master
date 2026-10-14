import { useEffect, useState } from 'react';
import type { MergePlan, TeamApi, TeamCard } from '../../../lib/team';
import { Sheet } from '../Sheet';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/**
 * "Merge #142 when its checks pass?" The plan is read from GitHub when the sheet opens (repository, title,
 * author, branch and the commit it is at), never from the card. Confirming arms GitHub's auto-merge at that
 * commit only. GitHub does the merge, not this app.
 */
export function MergeSheet({
  card,
  api,
  onClose,
  onDone,
}: {
  card: TeamCard;
  api: TeamApi;
  onClose: () => void;
  /** The sentence the backend returns, shown on the card. */
  onDone: (message: string) => void;
}) {
  const [plan, setPlan] = useState<MergePlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cardId = card.id;
  const number = card.event.subject.pr?.number;

  useEffect(() => {
    let cancelled = false;
    api
      .mergePlan(cardId)
      .then((p) => !cancelled && setPlan(p))
      .catch((err) => !cancelled && setError(errText(err)));
    return () => {
      cancelled = true;
    };
  }, [api, cardId]);

  const submit = async () => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      onDone(await api.mergeWhenGreen(cardId, plan.headSha));
    } catch (err) {
      setError(errText(err));
      setBusy(false);
    }
  };

  return (
    <Sheet labelledBy="ck-team-merge-t" role="dialog" onClose={busy ? undefined : onClose} className="ck-team-sheet">
      <h2 className="ck-section" id="ck-team-merge-t">
        Merge #{plan?.number ?? number} when its checks pass?
      </h2>
      {plan ? (
        <>
          <p className="ck-sub">{plan.title}</p>
          <p className="ck-team-meta ck-team-merge-facts">
            {plan.repo} · by {plan.author || 'unknown'} · {plan.headRef} → {plan.baseRef} · at commit {plan.headSha.slice(0, 7)}
          </p>
          <p className="ck-team-meta">
            GitHub merges it {plan.methodWords} once the required checks pass and branch protection allows it. If anyone pushes to it
            first, nothing is merged. Puppet Master never merges by itself.
          </p>
          {plan.autoMergeOff && (
            <p className="ck-team-meta">Auto-merge is off for {plan.repo}. An admin can turn it on in GitHub.</p>
          )}
        </>
      ) : (
        !error && <p className="ck-team-meta">Checking with GitHub…</p>
      )}
      {error && (
        <p className="ck-team-error" role="alert">
          {error}
        </p>
      )}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={onClose}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" disabled={busy || !plan || plan.autoMergeOff} onClick={() => void submit()}>
          Merge when green
        </button>
      </div>
    </Sheet>
  );
}
