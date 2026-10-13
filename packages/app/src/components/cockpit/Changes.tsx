import { useCallback, useEffect, useState } from 'react';
import { cockpitApi, type WorktreeDiff, type WorktreeItem } from '../../lib/cockpit-api';
import { lineKind, plural, splitPatch, statusMark } from './diff';
import '../../styles/cockpit-changes.css';

/** The slice of the cockpit API this section needs (a test double can stand in for it). */
export type ChangesApi = Pick<
  typeof cockpitApi,
  'paneWorktree' | 'worktreeDiff' | 'worktreeMerge' | 'worktreeDiscard'
>;

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/**
 * "Changes" in the Receipt tab: what an isolated agent changed in its git worktree, with Merge and
 * Discard. Renders nothing for a pane that is not working in a worktree.
 */
export function Changes({
  paneId,
  status,
  api = cockpitApi,
}: {
  paneId: string;
  status: string;
  api?: ChangesApi;
}) {
  const [item, setItem] = useState<WorktreeItem | null>(null);
  const [diff, setDiff] = useState<WorktreeDiff | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [sheet, setSheet] = useState<'merge' | 'discard' | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  if (!item) return notice ? <p className="ck-sub ck-pad">{notice}</p> : null;

  const files = diff?.changes.files ?? [];
  const bodies = diff ? splitPatch(diff.patch) : new Map<string, string[]>();
  const working = item.state === 'active';
  const total = diff?.changes;

  const run = async (action: 'merge' | 'discard') => {
    setBusy(true);
    setError(null);
    try {
      if (action === 'merge') {
        const out = await api.worktreeMerge(item.repo, item.handle);
        if (out.merged) {
          setNotice(`Merged ${out.branch} into ${out.into}.`);
          setItem(null);
        } else {
          setNotice('Nothing to merge: the agent changed no files.');
        }
      } else {
        await api.worktreeDiscard(item.repo, item.handle);
        setNotice('Discarded. The branch and its changes are deleted.');
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
        <span className="ck-changes-state">{working ? 'Working' : 'Unreviewed'}</span>
      </div>
      <p className="ck-changes-meta">
        <code>{item.branch}</code> from <code>{item.base}</code>
        {total && (
          <>
            {' · '}
            {plural(total.files_changed, 'file')}
            {' · '}
            <span className="ck-num">+{total.additions}</span> <span className="ck-num">−{total.deletions}</span>
          </>
        )}
      </p>

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
              </button>
              {isOpen && (
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
              )}
            </li>
          );
        })}
      </ul>
      {diff?.changes.truncated && <p className="ck-sub">Only the first {files.length} files are listed.</p>}

      {error && (
        <p className="ck-changes-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="ck-sub">{notice}</p>}

      <div className="ck-changes-actions">
        <button
          type="button"
          className="ck-btn ck-btn--primary"
          disabled={working || busy || item.state === 'missing'}
          title={working ? 'The agent is still working' : undefined}
          onClick={() => setSheet('merge')}
        >
          Merge
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
      </div>

      {sheet && (
        <div className="ck-scrim" onMouseDown={() => !busy && setSheet(null)}>
          <div className="ck-sheet" role="alertdialog" aria-labelledby="ck-wt-t" onMouseDown={(e) => e.stopPropagation()}>
            <h2 className="ck-section" id="ck-wt-t">
              {sheet === 'merge' ? `Merge into ${item.base}?` : 'Discard these changes?'}
            </h2>
            <p className="ck-sub">
              {sheet === 'merge'
                ? `Brings ${plural(total?.files_changed ?? 0, 'changed file')} from ${item.branch} into your project and closes this pane. If anything conflicts, nothing is merged.`
                : `Deletes ${item.branch} and ${plural(total?.files_changed ?? 0, 'changed file')}, and closes this pane. This cannot be undone.`}
            </p>
            <div className="ck-sheet-actions">
              <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={() => setSheet(null)}>
                Cancel
              </button>
              <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void run(sheet)}>
                {sheet === 'merge' ? 'Merge' : 'Discard'}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
