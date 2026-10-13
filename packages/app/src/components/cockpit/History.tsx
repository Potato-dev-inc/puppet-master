import { useCallback, useEffect, useRef, useState } from 'react';
import {
  cockpitApi,
  type Checkpoint,
  type CheckpointList,
  type CheckpointsApi,
  type FileChange,
  type RestoreReport,
} from '../../lib/cockpit-api';
import { checkpointTitle, formatAgo, formatDelta } from './history-model';

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/**
 * The History tab: one row per checkpoint, newest first. "Restore" puts the project's files back
 * to that point (after saving the current state as a checkpoint, so it can itself be undone).
 */
export function History({
  paneId,
  status,
  now,
  notify,
  api = cockpitApi,
}: {
  paneId: string;
  status: string;
  now: number;
  notify: (text: string) => void;
  api?: CheckpointsApi;
}) {
  const [listing, setListing] = useState<CheckpointList | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [diffs, setDiffs] = useState<Record<string, FileChange[] | string>>({});
  const [pending, setPending] = useState<Checkpoint | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);
  const running = status === 'running';

  const load = useCallback(async () => {
    try {
      const next = await api.checkpointsList(paneId);
      if (!alive.current) return;
      setListing(next);
      setError(null);
    } catch (e) {
      if (alive.current) setError(errText(e));
    }
  }, [api, paneId]);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    setListing(undefined);
    setOpen(null);
    setDiffs({});
    setPending(null);
  }, [paneId]);

  // A turn starting or ending is what creates checkpoints, so reload on every status change.
  useEffect(() => {
    void load();
  }, [load, status]);

  const toggle = (item: Checkpoint) => {
    if (open === item.id) {
      setOpen(null);
      return;
    }
    setOpen(item.id);
    if (diffs[item.id] !== undefined) return;
    api
      .checkpointDiff(paneId, item.id)
      .then((files) => alive.current && setDiffs((d) => ({ ...d, [item.id]: files })))
      .catch((e) => alive.current && setDiffs((d) => ({ ...d, [item.id]: errText(e) })));
  };

  const confirmRestore = async () => {
    if (!pending) return;
    const target = pending;
    setBusy(true);
    try {
      const report: RestoreReport = await api.checkpointRestore(paneId, target.id);
      setPending(null);
      const failed = report.failed.length ? ` ${report.failed.length} file(s) could not be changed.` : '';
      notify(`Files restored to "${checkpointTitle(target)}". Your previous files are saved in History.${failed}`);
      setDiffs({});
      await load();
    } catch (e) {
      setPending(null);
      notify(errText(e));
    } finally {
      setBusy(false);
    }
  };

  if (error) {
    return (
      <div className="ck-empty">
        <h2 className="ck-section">History unavailable</h2>
        <p className="ck-sub">{error}</p>
      </div>
    );
  }
  if (listing === undefined) return <p className="ck-sub ck-pad">Loading…</p>;
  if (listing.state === 'off' || listing.state === 'no_git') {
    return (
      <div className="ck-empty">
        <h2 className="ck-section">{listing.state === 'off' ? 'Checkpoints are off' : 'Git is needed'}</h2>
        <p className="ck-sub">
          {listing.state === 'off'
            ? 'Turn them on in Settings > General to save your files at the start and end of every turn.'
            : listing.message}
        </p>
      </div>
    );
  }
  if (listing.items.length === 0) {
    return (
      <div className="ck-empty">
        <h2 className="ck-section">No checkpoints yet</h2>
        <p className="ck-sub">
          Your files are saved automatically when this agent starts and finishes a turn. They will show up here, and you can
          go back to any of them.
        </p>
      </div>
    );
  }

  return (
    <div className="ck-scroll ck-fade-in">
      <p className="ck-hist-note">
        Restoring puts your files back. It doesn't change what the agent remembers.
        {running && ' The agent is working: interrupt it before restoring.'}
      </p>
      <ol className="ck-hist" aria-label="Checkpoints">
        {listing.items.map((item) => {
          const expanded = open === item.id;
          const diff = diffs[item.id];
          return (
            <li key={item.id} className="ck-hist-item" data-kind={item.kind}>
              <div className="ck-hist-row">
                <button type="button" className="ck-hist-main" aria-expanded={expanded} onClick={() => toggle(item)}>
                  <span className="ck-hist-time" title={new Date(item.atMs).toLocaleString()}>
                    {formatAgo(now - item.atMs)}
                  </span>
                  <span className="ck-hist-label">
                    <span className="ck-hist-title">{checkpointTitle(item)}</span>
                    {item.note && <span className="ck-hist-sub">{item.note}</span>}
                  </span>
                  <span className="ck-hist-delta">{formatDelta(item)}</span>
                </button>
                <button
                  type="button"
                  className="ck-btn ck-btn--sm ck-btn--secondary"
                  disabled={running || busy}
                  title={running ? 'Interrupt the agent first' : undefined}
                  onClick={() => setPending(item)}
                >
                  Restore
                </button>
              </div>
              {expanded && (
                <div className="ck-hist-files">
                  {diff === undefined ? (
                    <span className="ck-sub">Loading…</span>
                  ) : typeof diff === 'string' ? (
                    <span className="ck-sub">{diff}</span>
                  ) : diff.length === 0 ? (
                    <span className="ck-sub">No file changes.</span>
                  ) : (
                    <ul>
                      {diff.map((f) => (
                        <li key={f.path}>
                          <span className="ck-hist-status">{f.status}</span>
                          <span className="ck-hist-path">{f.path}</span>
                          <span className="ck-hist-delta">{f.binary ? 'binary' : `+${f.added} −${f.deleted}`}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ol>

      {pending && (
        <div className="ck-scrim" onMouseDown={() => !busy && setPending(null)}>
          <div className="ck-sheet" role="alertdialog" aria-labelledby="ck-restore-t" onMouseDown={(e) => e.stopPropagation()}>
            <h2 className="ck-section" id="ck-restore-t">
              Restore "{checkpointTitle(pending)}"?
            </h2>
            <p className="ck-sub">
              This restores your files. It doesn't change what the agent remembers. Files created since then are removed, and
              files in .gitignore are left alone.
            </p>
            <p className="ck-sub">Your current files are saved first, so you can undo this from History.</p>
            <div className="ck-sheet-actions">
              <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={() => setPending(null)}>
                Cancel
              </button>
              <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void confirmRestore()}>
                {busy ? 'Restoring…' : 'Restore files'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
