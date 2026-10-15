import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { cockpitApi, type WorktreeDiff, type WorktreeItem } from '../../lib/cockpit-api';
import { lineKind, plural, splitPatch, statusMark } from './diff';
import { secondOpinionApi, type SecondOpinionApi } from '../../lib/second-opinion';
import { AnnotatedDiff } from './AnnotatedDiff';
import { anchorFindings, reviewHasConcerns } from './review-diff';
import { sendToAuthor, useReviewRun } from './second-opinion-store';
import { Sheet } from './Sheet';
import { CreatePrSheet } from './CreatePrSheet';
import { PrBlock } from './PrBlock';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { shipApi, type PrRef, type ShipApi } from '../../lib/ship-api';
import { setupLine } from '../../lib/ship-model';
import { ChangedFiles } from '../simple/ChangedFiles';
import '../../styles/cockpit-changes.css';

/** The slice of the cockpit API this section needs (a test double can stand in for it). */
export type ChangesApi = Pick<
  typeof cockpitApi,
  'paneWorktree' | 'worktreeDiff' | 'worktreeMerge' | 'worktreeDiscard'
>;

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/** What a separate copy is called on screen: never its branch. */
export function copyHeadline(copyLabel: string | undefined, base: string): string {
  return copyLabel ?? `Separate copy of ${base}`;
}

/** Refusals that get a plain sentence; the raw text stays in the tooltip (`title`). */
export function plainMergeError(raw: string): { text: string; title?: string } {
  if (/ORIGIN_DIRTY_OVERLAP/.test(raw)) {
    const files = /\(([^()]*)\)\s*$/.exec(raw.trim())?.[1];
    const sentence = 'Your project has unsaved changes to the same files. Save or undo them first.';
    return { text: files ? `${sentence} (${files})` : sentence, title: raw };
  }
  if (/MERGE_CONFLICT/.test(raw)) {
    const files = /\(([^()]*)\)\s*$/.exec(raw.trim())?.[1];
    const sentence = 'Some files changed in both places, so nothing was merged. Your project is unchanged and the copy is safe.';
    return { text: files ? `${sentence} (${files})` : sentence, title: raw };
  }
  if (/ORIGIN_DETACHED/.test(raw)) {
    return { text: "Your project isn't on a branch. Switch to a branch, then merge.", title: raw };
  }
  if (/ORIGIN_MERGE_IN_PROGRESS/.test(raw)) {
    return { text: 'Your project is in the middle of a merge. Finish or cancel it first.', title: raw };
  }
  return { text: raw };
}

/**
 * "Changes" in the Receipt tab: what an isolated agent changed in its git worktree, with Merge and
 * Discard. Renders nothing for a pane that is not working in a worktree.
 */
export function Changes({
  paneId,
  status,
  api = cockpitApi,
  reviewApi = secondOpinionApi,
  secondOpinion,
  hideActions = false,
  ship = shipApi,
  copyLabel,
  plain = false,
}: {
  paneId: string;
  /** Simple mode: plain words, no branch, no diffs, no Merge / Discard (Simple owns those). */
  plain?: boolean;
  /** Show the files and diffs only (no Merge / Discard): another surface owns those decisions. */
  hideActions?: boolean;
  /** Shown in place of "Separate copy of <base>" when another surface owns the separate copy ("Separate copy of feat/billing"). */
  copyLabel?: string;
  status: string;
  api?: ChangesApi;
  /**
   * The Second opinion block. It renders between the file list and Merge / Discard, so the evidence
   * comes before the decision; without a worktree to merge it renders on its own.
   */
  secondOpinion?: ReactNode;
  /** Where "Send to <agent>" on a second-opinion note goes (a test double can stand in). */
  reviewApi?: SecondOpinionApi;
  /** Create PR and the GitHub checks (a test double can stand in). */
  ship?: ShipApi;
}) {
  const [item, setItem] = useState<WorktreeItem | null>(null);
  const [diff, setDiff] = useState<WorktreeDiff | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [sheet, setSheet] = useState<'merge' | 'discard' | null>(null);
  const [prSheet, setPrSheet] = useState(false);
  const [prRound, setPrRound] = useState(0);
  const listing = useAgentRegistry();
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // A finished second opinion on this worktree: its notes are pinned to the lines below.
  const review = useReviewRun(paneId);
  const reviewDone = review?.phase === 'done' && review.start?.source === 'worktree' ? review : null;
  const reviewHandle = reviewDone?.start?.handle;

  const load = useCallback(() => {
    let cancelled = false;
    api
      .paneWorktree(paneId)
      .then(async (found) => {
        if (cancelled) return;
        setItem(found);
        if (found && found.exists) {
          const d = await api.worktreeDiff(found.repo, found.handle);
          if (!cancelled) setDiff(d);
        } else {
          setDiff(null);
        }
      })
      .catch(() => {
        // Not in the desktop shell, or the lookup failed: there is nothing to review.
        if (!cancelled) setItem(null);
      });
    return () => {
      cancelled = true;
    };
  }, [api, paneId]);

  useEffect(() => {
    setOpen(null);
    setNotice(null);
    setError(null);
    return load();
  }, [load, status]);

  // Open the first file with a note once per review, so the notes are in view when they arrive.
  const openedFor = useRef<string | null>(null);
  useEffect(() => {
    if (!reviewHandle || !diff || !reviewDone?.review || openedFor.current === reviewHandle) return;
    openedFor.current = reviewHandle;
    const first = [...anchorFindings(reviewDone.review.findings, splitPatch(diff.patch)).byFile.keys()][0];
    if (first) setOpen(first);
  }, [reviewHandle, diff, reviewDone]);

  if (!item) {
    return (
      <>
        {notice && <p className="ck-sub ck-pad">{notice}</p>}
        {secondOpinion}
      </>
    );
  }

  const files = diff?.changes.files ?? [];
  const bodies = diff ? splitPatch(diff.patch) : new Map<string, string[]>();
  const working = item.state === 'active';
  if (plain) return <ChangedFiles files={files} working={working} loaded={diff !== null} />;
  const total = diff?.changes;
  const notes = reviewDone?.review ? anchorFindings(reviewDone.review.findings, bodies).byFile : null;
  // A review that found problems: Merge stops being the obvious button and says what it is.
  const concerns = reviewHasConcerns(reviewDone?.review ?? null);

  const setup = item.setup ?? null;
  // A copy whose setup failed could not run its checks: Merge is not the obvious next step.
  const setupFailed = setup?.status === 'failed';
  const agentName = item.agent_type ? agentLabel(listing, item.agent_type) : 'the agent';

  const run = async (action: 'merge' | 'discard') => {
    setBusy(true);
    setError(null);
    try {
      if (action === 'merge') {
        const out = await api.worktreeMerge(item.repo, item.handle);
        if (out.merged) {
          setNotice(`Merged into ${out.into}.`);
          setItem(null);
        } else {
          setNotice('Nothing to merge: the agent changed no files.');
        }
      } else {
        await api.worktreeDiscard(item.repo, item.handle);
        setNotice('Discarded. The separate copy and its changes are deleted.');
        setItem(null);
      }
      setSheet(null);
    } catch (err) {
      setSheet(null);
      setError(errText(err));
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="ck-changes" aria-label="Changes">
      <div className="ck-changes-head">
        <h2 className="ck-section">Changes</h2>
        <span className="ck-changes-state">{working ? 'Working' : 'Not merged yet'}</span>
      </div>
      <p className="ck-changes-meta">
        {copyHeadline(copyLabel, item.base)}
        {total && (
          <>
            {' · '}
            {plural(total.files_changed, 'file')}
            {' · '}
            <span className="ck-num">+{total.additions}</span> <span className="ck-num">−{total.deletions}</span>
          </>
        )}
      </p>

      {!copyLabel && (
        <details className="ck-details ck-changes-details">
          <summary>Details</summary>
          <dl className="ck-facts ck-facts--details">
            <div className="ck-facts-row">
              <dt>Branch</dt>
              <dd className="ck-mono">{item.branch}</dd>
            </div>
            <div className="ck-facts-row">
              <dt>Based on</dt>
              <dd className="ck-mono">{item.base}</dd>
            </div>
            <div className="ck-facts-row">
              <dt>Folder</dt>
              <dd className="ck-mono">{item.path}</dd>
            </div>
          </dl>
        </details>
      )}

      {setup && (
        <div className={`ck-changes-setup${setup.status === 'failed' ? ' is-failed' : ''}`}>
          <p className="ck-sub" role={setup.status === 'failed' ? 'alert' : undefined}>
            {setupLine(setup)}
          </p>
          {setup.status === 'failed' && setup.steps.some((st) => st.tail) && (
            <details className="ck-pr-details">
              <summary>Last lines of the output</summary>
              <pre className="ck-pr-command">{setup.steps.find((st) => !st.ok)?.tail}</pre>
            </details>
          )}
        </div>
      )}
      {item.pr && (
        <PrBlock
          key={prRound}
          repo={item.repo}
          handle={item.handle}
          pr={item.pr}
          agentName={agentName}
          ship={ship}
          onPr={(next: PrRef) => setItem((cur) => (cur ? { ...cur, pr: next } : cur))}
        />
      )}

      {diff && files.length === 0 && <p className="ck-sub">No file changes yet.</p>}
      <ul className="ck-changes-files">
        {files.map((f) => {
          const lines = bodies.get(f.path);
          const isOpen = open === f.path;
          return (
            <li key={f.path}>
              <button
                type="button"
                className={`ck-changes-file${isOpen ? ' is-open' : ''}`}
                aria-expanded={isOpen}
                onClick={() => setOpen(isOpen ? null : f.path)}
              >
                <span className="ck-changes-mark" aria-label={f.status}>
                  {statusMark(f.status)}
                </span>
                <span className="ck-changes-path">{f.path}</span>
                <span className="ck-changes-counts">{f.binary ? 'binary' : `+${f.additions} −${f.deletions}`}</span>
                {(notes?.get(f.path)?.length ?? 0) > 0 && (
                  <span className="ck-changes-notes">{plural(notes?.get(f.path)?.length ?? 0, 'note')}</span>
                )}
              </button>
              {isOpen && reviewDone?.start ? (
                <AnnotatedDiff
                  file={f.path}
                  lines={lines ?? []}
                  findings={notes?.get(f.path) ?? []}
                  agentLabel={reviewDone.start.authorLabel}
                  onSend={(file, line, text) => sendToAuthor(paneId, file, line, text, reviewApi).then(() => undefined)}
                  emptyText={diff?.patch_truncated ? 'Diff too large to show here.' : 'No textual changes.'}
                />
              ) : (
                isOpen && (
                  <pre className="ck-diff" aria-label={`Diff of ${f.path}`}>
                    {lines && lines.length > 0 ? (
                      lines.map((line, i) => (
                        <span key={i} className={`ck-diff-line ck-diff-${lineKind(line)}`}>
                          {line || ' '}
                        </span>
                      ))
                    ) : (
                      <span className="ck-diff-line ck-diff-ctx">
                        {diff?.patch_truncated ? 'Diff too large to show here.' : 'No textual changes.'}
                      </span>
                    )}
                  </pre>
                )
              )}
            </li>
          );
        })}
      </ul>
      {diff?.changes.truncated && <p className="ck-sub">Only the first {files.length} files are listed.</p>}

      {error && (
        <p className="ck-changes-error" role="alert" title={plainMergeError(error).title}>
          {plainMergeError(error).text}
        </p>
      )}
      {notice && <p className="ck-sub">{notice}</p>}

      {secondOpinion}

      {!hideActions && (
      <div className="ck-changes-actions">
        <button
          type="button"
          className={`ck-btn ${concerns || setupFailed ? 'ck-btn--secondary' : 'ck-btn--primary'}`}
          disabled={working || busy || item.state === 'missing'}
          title={working ? 'The agent is still working' : undefined}
          onClick={() => setSheet('merge')}
        >
          {concerns ? 'Merge anyway' : 'Merge'}
        </button>
        <button
          type="button"
          className="ck-btn ck-btn--secondary"
          disabled={working || busy || item.state === 'missing'}
          title={working ? 'The agent is still working' : undefined}
          onClick={() => setPrSheet(true)}
        >
          {item.pr ? 'Update PR' : 'Create PR'}
        </button>
        <button
          type="button"
          className="ck-btn ck-btn--secondary"
          disabled={working || busy}
          title={working ? 'The agent is still working' : undefined}
          onClick={() => setSheet('discard')}
        >
          Discard
        </button>
        {setupFailed && <span className="ck-changes-hint">Setup failed, checks didn't run.</span>}
      </div>
      )}

      {prSheet && (
        <CreatePrSheet
          item={item}
          ship={ship}
          agentName={agentName}
          onClose={() => setPrSheet(false)}
          onDone={(pr, updated) => {
            setPrSheet(false);
            setItem({ ...item, pr: { ...pr, checks: undefined } });
            setPrRound((r) => r + 1);
            setNotice(updated ? 'Pushed the new commits to the pull request.' : `Draft pull request #${pr.number} created.`);
          }}
        />
      )}

      {sheet && (
        <Sheet labelledBy="ck-wt-t" onClose={busy ? undefined : () => setSheet(null)}>
            <h2 className="ck-section" id="ck-wt-t">
              {sheet === 'merge' ? `Merge into ${item.base}?` : 'Discard this separate copy?'}
            </h2>
            <p className="ck-sub">
              {sheet === 'merge'
                ? `Brings ${plural(total?.files_changed ?? 0, 'changed file')} from this separate copy into ${item.base} and closes ${agentName}. If anything conflicts, nothing is merged.`
                : `Deletes this separate copy and its ${plural(total?.files_changed ?? 0, 'changed file')}, and closes ${agentName}. This cannot be undone.`}
            </p>
            <div className="ck-sheet-actions">
              <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={() => setSheet(null)}>
                Cancel
              </button>
              <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void run(sheet)}>
                {sheet === 'merge' ? 'Merge' : 'Discard'}
              </button>
            </div>
        </Sheet>
      )}
    </section>
  );
}
