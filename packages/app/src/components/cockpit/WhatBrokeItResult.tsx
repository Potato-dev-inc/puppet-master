// What broke it: the result card (SPEC n33 section 3.7, item 3). One view per finding kind. The undo confirm is
// a Sheet, like RestoreSheet. Test output and file names are shown as plain text only (React text nodes).
// Remount per finding (the parent keys it by finding id): the undo and check state start from the finding.
import { useEffect, useRef, useState } from 'react';
import {
  kindOf,
  wbDiff,
  wbFinding,
  wbRecheck,
  wbStatus,
  wbUndo,
  wbUndoPlan,
  type DiffView,
  type FindingView,
  type JobView,
  type UndoPlanView,
  type UndoReportView,
} from '../../lib/what-broke-it';
import type { FileChange } from '../../lib/cockpit-api';
import { sharedFolderWarning } from './history-model';
import type { Notify } from './restore-flow';
import { Sheet } from './Sheet';
import {
  brokeLine,
  canKeepLooking,
  countFiles,
  detailsText,
  recheckLine,
  suspectTitle,
  undoButton,
  undoRefusedText,
  undoText,
  undoTitle,
  undoneLine,
} from './what-broke-it-result-text';

/** The calls this card makes. The sheet passes its own object with the same names. */
export interface WhatBrokeItResultApi {
  wbDiff: (findingId: string, path?: string | null) => Promise<DiffView>;
  wbUndoPlan: (findingId: string) => Promise<UndoPlanView>;
  wbUndo: (findingId: string) => Promise<UndoReportView>;
  wbRecheck: (findingId: string) => Promise<JobView>;
  wbStatus: (jobId: string) => Promise<JobView>;
  wbFinding: (findingId: string) => Promise<FindingView>;
}

const liveApi: WhatBrokeItResultApi = { wbDiff, wbUndoPlan, wbUndo, wbRecheck, wbStatus, wbFinding };

/** The message of a rejected call, as plain text. */
function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function WhatBrokeItResult({
  finding,
  api = liveApi,
  sharedWith = [],
  notify,
  onKeepLooking,
}: {
  finding: FindingView;
  api?: WhatBrokeItResultApi;
  /** Labels of the other live panes working in this folder. */
  sharedWith?: string[];
  notify: Notify;
  onKeepLooking: () => void;
}) {
  const kind = kindOf(finding);
  const suspect = finding.suspects[0] ?? null;
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const [undoneAtMs, setUndoneAtMs] = useState<number | null>(finding.undoneAtMs);
  const [note, setNote] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // Show what it changed: the file list of the culprit turn, then one file's diff.
  const [showFiles, setShowFiles] = useState(false);
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const [patch, setPatch] = useState<{ path: string; text: string; truncated: boolean } | null>(null);

  // Undo only this turn: the dry run decides the confirm text (plain text, or the refusal that replaces it).
  const [confirming, setConfirming] = useState(false);
  const [plan, setPlan] = useState<UndoPlanView | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [undoBusy, setUndoBusy] = useState(false);

  // Check it again: one run after the undo, then the finding it made.
  const [checking, setChecking] = useState(false);
  const [checkedLine, setCheckedLine] = useState<string | null>(null);

  const canUndo = kind === 'culprit' && finding.canUndo && undoneAtMs === null;
  const warning = sharedFolderWarning(sharedWith);
  const undoReady = plan !== null && refusal === null && !undoBusy;

  const toggleFiles = () => {
    if (showFiles) {
      setShowFiles(false);
      return;
    }
    setShowFiles(true);
    if (files !== null) return;
    api.wbDiff(finding.id).then(
      (view) => alive.current && setFiles(view.files),
      (e) => alive.current && setNote(messageOf(e)),
    );
  };

  const openFile = (path: string) => {
    api.wbDiff(finding.id, path).then(
      (view) => alive.current && setPatch({ path, text: view.patch ?? '', truncated: view.truncated }),
      (e) => alive.current && setNote(messageOf(e)),
    );
  };

  const openUndo = () => {
    setConfirming(true);
    setPlan(null);
    setRefusal(null);
    api.wbUndoPlan(finding.id).then(
      (next) => {
        if (!alive.current) return;
        setPlan(next);
        if (next.blockedBy) setRefusal(next.blockedBy);
        else if (next.conflicts.length > 0) setRefusal(undoRefusedText(next.conflicts.map((c) => c.path)));
      },
      (e) => alive.current && setRefusal(messageOf(e)),
    );
  };

  const confirmUndo = () => {
    setUndoBusy(true);
    api.wbUndo(finding.id).then(
      (report) => {
        if (!alive.current) return;
        setUndoneAtMs(report.undoneAtMs);
        setConfirming(false);
      },
      (e) => alive.current && setRefusal(messageOf(e)),
    ).finally(() => alive.current && setUndoBusy(false));
  };

  const checkAgain = async () => {
    setChecking(true);
    setCheckedLine(null);
    try {
      let job = await api.wbRecheck(finding.id);
      while (job.state !== 'done') {
        await new Promise((resolve) => setTimeout(resolve, 400));
        if (!alive.current) return;
        job = await api.wbStatus(job.id);
      }
      if (!job.findingId) {
        if (alive.current) setCheckedLine('The check did not finish.');
        return;
      }
      const next = await api.wbFinding(job.findingId);
      if (alive.current) setCheckedLine(recheckLine(next));
    } catch (e) {
      if (alive.current) setNote(messageOf(e));
    } finally {
      if (alive.current) setChecking(false);
    }
  };

  const copyDetails = () => {
    const text = detailsText(finding);
    if (!navigator.clipboard) {
      notify("Couldn't copy the details.");
      return;
    }
    navigator.clipboard.writeText(text).then(
      () => alive.current && setCopied(true),
      () => notify("Couldn't copy the details."),
    );
  };

  return (
    <section className="ck-wb-result">
      <h2 className="ck-section">{finding.title}</h2>
      {finding.subtitle && <p className="ck-sub">{finding.subtitle}</p>}

      {kind === 'culprit' && <p className="ck-sub">{brokeLine(finding.failing)}</p>}

      {kind === 'several' && (
        <>
          <ul className="ck-list">
            {finding.suspects.map((s) => (
              <li key={`${s.paneKey}-${s.turn}`} className="ck-row">
                <span className="ck-row-title">{suspectTitle(s)}</span>
                {s.sentence && <span className="ck-row-sub">{s.sentence}</span>}
              </li>
            ))}
          </ul>
          <p className="ck-sub">I can&apos;t tell which one.</p>
        </>
      )}

      {showFiles && files !== null && (
        <ul className="ck-list">
          {files.length === 0 && <li className="ck-sub">No files changed in this turn.</li>}
          {files.map((f) => (
            <li key={f.path} className="ck-row">
              <button type="button" className="ck-btn ck-btn--plain" onClick={() => openFile(f.path)}>
                {f.path}
              </button>
              <span className="ck-mono">{f.binary ? 'binary' : `+${f.added} −${f.deleted}`}</span>
            </li>
          ))}
        </ul>
      )}
      {showFiles && patch && (
        <>
          <p className="ck-sub">{patch.path}</p>
          {patch.text ? <pre className="ck-diff ck-mono">{patch.text}</pre> : <p className="ck-sub">No text change to show.</p>}
          {patch.truncated && <p className="ck-sub">The diff is cut short.</p>}
        </>
      )}

      {undoneAtMs !== null && <p className="ck-sub">{undoneLine(undoneAtMs)}</p>}
      {checkedLine && <p className="ck-sub">{checkedLine}</p>}
      {note && <p className="ck-sub">{note}</p>}

      <div className="ck-sheet-actions">
        {kind === 'culprit' && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={toggleFiles}>
            Show what it changed
          </button>
        )}
        {canUndo && (
          <button type="button" className="ck-btn ck-btn--primary" onClick={openUndo}>
            Undo only this turn
          </button>
        )}
        {undoneAtMs !== null && kind === 'culprit' && (
          <button type="button" className="ck-btn ck-btn--secondary" disabled={checking} onClick={() => void checkAgain()}>
            {checking ? 'Checking…' : 'Check it again'}
          </button>
        )}
        {canKeepLooking(finding) && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={onKeepLooking}>
            Keep looking
          </button>
        )}
        <button type="button" className="ck-btn ck-btn--plain" onClick={copyDetails}>
          Copy details
        </button>
        {copied && <span className="ck-sub">Copied.</span>}
      </div>

      {confirming && suspect && (
        <Sheet labelledBy="ck-wb-undo-t" onClose={undoBusy ? undefined : () => setConfirming(false)}>
          <h2 className="ck-section" id="ck-wb-undo-t">
            {undoTitle(suspect.turn)}
          </h2>
          <p className="ck-sub">
            {refusal ?? undoText(suspect.turn, suspect.files ? suspect.files.count : null, suspect.agentLabel)}
          </p>
          {!refusal && warning && <p className="ck-sub">{warning}</p>}
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={undoBusy} onClick={() => setConfirming(false)}>
              Cancel
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={!undoReady} onClick={confirmUndo}>
              {undoBusy ? 'Undoing…' : undoButton(suspect.turn)}
            </button>
          </div>
        </Sheet>
      )}
    </section>
  );
}
