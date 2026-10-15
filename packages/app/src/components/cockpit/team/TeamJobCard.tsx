import { useMemo, useState } from 'react';
import type { WorktreeItem } from '../../../lib/cockpit-api';
import { jobSub, jobTitle, type TeamApi, type TeamJob } from '../../../lib/team';
import { Changes, type ChangesApi } from '../Changes';
import { CardButton, InboxCard, type CardGlyphKind } from '../InboxCard';
import { Sheet } from '../Sheet';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

function glyph(state: TeamJob['state']): CardGlyphKind {
  switch (state) {
    case 'working':
      return 'working';
    case 'failed':
      return 'error';
    case 'ready':
    case 'applied':
      return 'done';
    default:
      return 'notice';
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
 * Apply to my branch (fast-forward only, never a push) / See changes / Discard. When the merge
 * stopped on conflicts and the agent would run commands without asking, it waits for Start
 * (it then opens in a pane I can watch).
 */
export function TeamJobCard({ job, api, paneName, onDone }: { job: TeamJob; api: TeamApi; paneName?: string; onDone?: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [showChanges, setShowChanges] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const changesApi = useMemo(() => changesApiFor(job, api), [job, api]);
  const ready = job.state === 'ready';
  const needsStart = job.state === 'needs_start';
  const canDiscard = job.state === 'ready' || job.state === 'failed' || job.state === 'waiting' || needsStart;

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
    <>
      <InboxCard
        kind="team"
        glyph={glyph(job.state)}
        title={jobTitle(job)}
        reason={jobSub(job, paneName)}
        data={{ 'data-team-job': job.id, 'data-state': job.state }}
        body={
          <>
            {ready && job.conflicts.length > 0 && <p className="ib-note">Resolved: {job.conflicts.join(', ')}</p>}
            {notice && notice !== job.message && <p className="ib-note">{notice}</p>}
            {error && (
              <p className="ib-error" role="alert">
                {error}
              </p>
            )}
            {needsStart && job.conflicts.length > 0 && <p className="ib-note">Conflicts: {job.conflicts.join(', ')}</p>}
            {showChanges && ready && (
              <div className="ck-team-changes">
                <Changes paneId={`team:${job.id}`} status={job.state} api={changesApi} hideActions copyLabel={`Separate copy of ${job.into}`} />
              </div>
            )}
          </>
        }
        actions={
          ready || canDiscard ? (
            <>
              {needsStart && (
                <CardButton tone="secondary" role="answer" disabled={busy} onClick={() => void run(() => api.startJob(job.id))}>
                  Start {job.agentLabel}
                </CardButton>
              )}
              {ready && (
                <CardButton tone="secondary" role="answer" disabled={busy} onClick={() => void run(() => api.apply(job.id))}>
                  Apply to my branch
                </CardButton>
              )}
              {ready && (
                <CardButton tone="secondary" aria-expanded={showChanges} onClick={() => setShowChanges((v) => !v)}>
                  {showChanges ? 'Hide changes' : 'See changes'}
                </CardButton>
              )}
            </>
          ) : null
        }
        more={
          canDiscard ? (
            <CardButton tone="plain" role="dismiss" disabled={busy} onClick={() => setConfirmDiscard(true)}>
              Discard
            </CardButton>
          ) : null
        }
      />
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
    </>
  );
}
