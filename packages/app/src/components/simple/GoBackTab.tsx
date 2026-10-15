// Go back: earlier versions of her project, newest first. It reads the checkpoints PM already
// saves after every step (the History tab's data) and restores them through the same call, so
// there is one undo, drawn twice. No hashes, no turn numbers, no Git words.
import { useCallback, useEffect, useRef, useState } from 'react';
import { cockpitApi, type CheckpointList, type CheckpointsApi, type TurnSummary } from '../../lib/cockpit-api';
import { restoreWithUndo, type Notify, type RestoreWords } from '../cockpit/restore-flow';
import { Sheet } from '../cockpit/Sheet';
import { F, T } from './copy';
import { agoText, clockText, versionsFrom, type Version } from './go-back-model';

const WORDS: RestoreWords = {
  restored: (target) => F.wentBackTo(target.label),
  undo: T.goBackUndo,
  undone: T.goBackUndone,
};

export function GoBackTab({
  paneId,
  status,
  now,
  agent,
  notify,
  api = cockpitApi,
}: {
  paneId: string;
  status: string;
  now: number;
  agent: string;
  notify: Notify;
  api?: CheckpointsApi & { turnSummaries: (paneId: string) => Promise<TurnSummary[]> };
}) {
  const [listing, setListing] = useState<CheckpointList | null | undefined>(undefined);
  const [summaries, setSummaries] = useState<TurnSummary[]>([]);
  const [pending, setPending] = useState<Version | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);
  const working = status === 'running';

  const load = useCallback(async () => {
    try {
      const [next, sums] = await Promise.all([api.checkpointsList(paneId), api.turnSummaries(paneId).catch(() => [])]);
      if (!alive.current) return;
      setListing(next);
      setSummaries(sums);
    } catch {
      if (alive.current) setListing(null);
    }
  }, [api, paneId]);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  // A step starting or ending is what saves a version, so look again on every state change.
  useEffect(() => {
    void load();
  }, [load, status]);

  if (listing === undefined) return <p className="sx-quiet sx-pad">{T.thinking}</p>;
  if (listing === null) return <p className="sx-quiet sx-pad">{T.goBackUnavailable}</p>;
  if (listing.state === 'off') return <p className="sx-quiet sx-pad">{T.goBackOff}</p>;
  if (listing.state === 'no_git') return <p className="sx-quiet sx-pad">{T.goBackNeedsHelper}</p>;
  const versions = versionsFrom(listing.items, summaries, agent, working);
  if (versions.length === 0) return <p className="sx-quiet sx-pad">{T.goBackEmpty}</p>;

  const confirm = async () => {
    if (!pending) return;
    const target = pending;
    setBusy(true);
    // The restore's own error text is for a developer; she gets one plain sentence.
    const plainApi = {
      checkpointRestore: (id: string, cp: string) =>
        api.checkpointRestore(id, cp).catch(() => {
          throw new Error(T.goBackUnavailable);
        }),
    };
    const report = await restoreWithUndo(plainApi, paneId, { id: target.id, label: clockText(target.atMs) }, notify, WORDS);
    setPending(null);
    if (report) await load();
    if (alive.current) setBusy(false);
  };

  return (
    <>
      <div className="sx-scroll">
        <p className="sx-lead">{T.goBackIntro}</p>
        {working && <p className="sx-quiet">{T.goBackWorking}</p>}
        <ol className="sx-list sx-versions" aria-label={T.tabGoBack}>
          {versions.map((v) => (
            <li key={v.id} className="sx-row sx-row--version">
              <span className="sx-row-main">
                <span className="sx-row-title">{v.current ? T.goBackNow : clockText(v.atMs)}</span>
                <span className="sx-row-sub">
                  {v.title} · {agoText(now - v.atMs)}
                </span>
              </span>
              {!v.current && (
                <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" disabled={working || busy} onClick={() => setPending(v)}>
                  {T.goBackHere}
                </button>
              )}
            </li>
          ))}
        </ol>
      </div>
      {pending && (
        <Sheet labelledBy="sx-back-t" onClose={busy ? undefined : () => setPending(null)}>
          <h2 className="ck-section" id="sx-back-t">
            {F.goBackConfirmTitle(clockText(pending.atMs))}
          </h2>
          <p className="ck-sub">{T.goBackConfirmBody}</p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={() => setPending(null)}>
              {T.cancel}
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void confirm()}>
              {T.goBackConfirm}
            </button>
          </div>
        </Sheet>
      )}
    </>
  );
}
