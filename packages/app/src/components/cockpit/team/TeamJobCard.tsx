import { useMemo, useState } from 'react';
import type { WorktreeItem } from '../../../lib/cockpit-api';
import { jobSub, jobTitle, type TeamApi, type TeamJob } from '../../../lib/team';
import { Changes, type ChangesApi } from '../Changes';
import { DotSpinner } from '../DotSpinner';
import type { SpinnerStatus } from '../model';
import { Sheet } from '../Sheet';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

function glyph(state: TeamJob['state']): SpinnerStatus {
  switch (state) {
    case 'working':
      return 'running';
    case 'failed':
      return 'error';
    case 'ready':
    case 'applied':
      return 'exited';
    default:
      return 'idle';
  }
}

/** The separate copy as the Changes view reads it (it is looked up by job, not by pane). */
function changesApiFor(job: TeamJob, api: TeamApi): ChangesApi {
  const item: WorktreeItem = {
    handle: job.handle,
    branch: job.copyBranch ?? '',
    base: job.into,
    repo: job.project,
    path: '',
    exists: true,
    state: 'unreviewed',
    summary: job.stats ? { files_changed: job.stats.files, additions: job.stats.additions, deletions: job.stats.deletions } : null,
  };
  return {
    paneWorktree: async () => item,
    worktreeDiff: async () => api.diff(job.id),
    worktreeMerge: async () => {
      throw new Error('Use Apply on the card.');
    },
    worktreeDiscard: async () => {
      throw new Error('Use Discard on the card.');
    },
  };
}

/**
 * A "Queue my agent" update: waiting for the pane's turn, merging, then the result with
 * Apply to my branch (fast-forward only, never a push) / See changes / Discard.
 */
export function TeamJobCard({ job, api, paneName, onDone }: { job: TeamJob; api: TeamApi; paneName?: string; onDone?: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [showChanges, setShowChanges] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const changesApi = useMemo(() => changesApiFor(job, api), [job, api]);
  const ready = job.state === 'ready';
  const canDiscard = job.state === 'ready' || job.state === 'failed' || job.state === 'waiting';

  const run = async (what: () => Promise<string | void>) => {
    setBusy(true);
    setError(null);
    try {
      const out = await what();
      if (typeof out === 'string') setNotice(out);
      setConfirmDiscard(false);
      onDone?.();
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="ck-inbox-item ck-team-job" data-team-job={job.id} data-state={job.state}>
      <div className="ck-inbox-top">
        <span className="ck-row-glyph">
          <DotSpinner status={glyph(job.state)} />
        </span>
        <span className="ck-row-title">{jobTitle(job)}</span>
      </div>
      <p className="ck-team-meta">{jobSub(job, paneName)}</p>
      {ready && job.conflicts.length > 0 && <p className="ck-team-meta">Resolved: {job.conflicts.join(', ')}</p>}
      {notice && notice !== job.message && <p className="ck-team-meta">{notice}</p>}
      {error && (
        <p className="ck-team-error" role="alert">
          {error}
        </p>
      )}
      {(ready || canDiscard) && (
        <div className="ck-inbox-actions">
          {ready && (
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void run(() => api.apply(job.id))}>
              Apply to my branch
            </button>
          )}
          {ready && (
            <button type="button" className="ck-btn ck-btn--secondary" aria-expanded={showChanges} onClick={() => setShowChanges((v) => !v)}>
              {showChanges ? 'Hide changes' : 'See changes'}
            </button>
          )}
          {canDiscard && (
            <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => setConfirmDiscard(true)}>
              Discard
            </button>
          )}
        </div>
      )}
      {showChanges && ready && (
        <div className="ck-team-changes">
          <Changes paneId={`team:${job.id}`} status={job.state} api={changesApi} hideActions copyLabel={`Separate copy of ${job.into}`} />
        </div>
      )}
      {confirmDiscard && (
        <Sheet labelledBy="ck-team-discard-t" onClose={busy ? undefined : () => setConfirmDiscard(false)}>
          <h2 className="ck-section" id="ck-team-discard-t">
            Discard this update?
          </h2>
          <p className="ck-sub">Deletes the separate copy and what the agent did in it. Your branch stays as it is.</p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={() => setConfirmDiscard(false)}>
              Cancel
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void run(() => api.discard(job.id))}>
              Discard
            </button>
          </div>
        </Sheet>
      )}
    </li>
  );
}
