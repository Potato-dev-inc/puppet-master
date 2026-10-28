// Branch from here: the rows above a chat that a branch touches. Dense, monochrome, the Keep going row classes
// (`ck-chat-note`, `ck-btn--plain`). Rows, by what this pane is:
//   new pane:     "Branched from Claude Code · after turn 5 · 14:02 · Show brief · Open original"
//   source pane:  "Branches: Codex · Cursor Agent (couldn't start)"   (each name opens its pane)
//   making:       "Making a copy from the save point…"
//   failed:       "Branch couldn't start: <state text>"
//   partial:      "3 files couldn't be written into the copy"
// It reads `branchLineage` and reloads when `branch://changed` names this pane. Nothing here starts a branch.
import { Fragment, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import {
  branchBrief,
  branchLineage,
  branchStateText,
  onBranchChanged,
  type BranchLineage,
  type BranchView,
} from '../../lib/branch-from';
import { BriefSheet } from '../cockpit/BriefSheet';
import { plainKeepGoingError } from './KeepGoingBanner';

const ROW_STYLE: CSSProperties = { display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' };

/** The short note after a branch's name on its source pane. Empty for a branch that is still open. */
export function branchSuffix(view: BranchView): string {
  if (view.state === 'failed') return "(couldn't start)";
  if (view.closed === 'kept') return '(kept)';
  if (view.closed === 'thrown_away') return '(thrown away)';
  return '';
}

const Dot = () => <span aria-hidden="true">·</span>;

function Row({ kind, children }: { kind: string; children: ReactNode }) {
  return (
    <div className="ck-chat-note" role="status" data-branch-banner={kind} style={ROW_STYLE}>
      {children}
    </div>
  );
}

export function BranchBanner({ paneId, onOpenPane }: { paneId: string; onOpenPane?: (paneId: string) => void }) {
  const [lineage, setLineage] = useState<BranchLineage | null>(null);
  const [brief, setBrief] = useState<{ agent: string; text: string | null; error: string | null } | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    let unlisten: (() => void) | undefined;
    const load = () => {
      branchLineage(paneId).then(
        (next) => {
          if (alive.current) setLineage(next);
        },
        () => {
          if (alive.current) setLineage(null);
        },
      );
    };
    load();
    onBranchChanged((event) => {
      if (event.paneIds.includes(paneId)) load();
    }).then(
      (stop) => {
        if (alive.current) unlisten = stop;
        else stop();
      },
      // No event channel (a test stub, a window without it): the rows still show what they loaded.
      () => {},
    );
    return () => {
      alive.current = false;
      unlisten?.();
    };
  }, [paneId]);

  const from = lineage?.from ?? null;
  const children = lineage?.children ?? [];
  if (!from && children.length === 0) return null;

  const making = children.some((c) => c.state === 'preparing');
  const failed = children.find((c) => c.state === 'failed') ?? null;
  const partial = [from, ...children].find((v): v is BranchView => v !== null && v.state !== 'failed' && v.failedFiles > 0) ?? null;

  const showBrief = (view: BranchView) => {
    const agent = view.toLabel;
    setBrief({ agent, text: null, error: null });
    branchBrief(view.id).then(
      (text) => {
        if (alive.current) setBrief({ agent, text, error: null });
      },
      (err: unknown) => {
        if (alive.current) setBrief({ agent, text: null, error: plainKeepGoingError(err) });
      },
    );
  };

  // A name is a button only when its pane exists and the window can open it.
  const nameOf = (view: BranchView): ReactNode => {
    const text = [view.toLabel, branchSuffix(view)].filter(Boolean).join(' ');
    if (view.toPaneId && onOpenPane) {
      return (
        <button type="button" className="ck-btn ck-btn--plain" onClick={() => onOpenPane(view.toPaneId)}>
          {text}
        </button>
      );
    }
    return <span>{text}</span>;
  };

  return (
    <>
      {from && (
        <Row kind="new">
          <span>{`Branched from ${from.fromLabel} · ${from.pointLabel}`}</span>
          <Dot />
          <button type="button" className="ck-btn ck-btn--plain" onClick={() => showBrief(from)}>
            Show brief
          </button>
          {onOpenPane && (
            <button type="button" className="ck-btn ck-btn--plain" onClick={() => onOpenPane(from.fromPaneId)}>
              Open original
            </button>
          )}
        </Row>
      )}
      {making && <Row kind="making">Making a copy from the save point…</Row>}
      {failed && <Row kind="failed">{`Branch couldn't start: ${branchStateText(failed)}`}</Row>}
      {partial && (
        <Row kind="partial">
          {`${partial.failedFiles} ${partial.failedFiles === 1 ? 'file' : 'files'} couldn't be written into the copy`}
        </Row>
      )}
      {children.length > 0 && (
        <Row kind="source">
          <span>Branches:</span>
          {children.map((c, i) => (
            <Fragment key={c.id}>
              {i > 0 && <Dot />}
              {nameOf(c)}
            </Fragment>
          ))}
        </Row>
      )}
      {brief && (
        <BriefSheet agent={brief.agent} text={brief.text} error={brief.error} onClose={() => setBrief(null)} />
      )}
    </>
  );
}
