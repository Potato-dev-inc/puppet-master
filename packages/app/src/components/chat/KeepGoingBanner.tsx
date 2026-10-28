// The Keep Going line above a pane's chat. One dense row, shown only when this pane was handed over to, or
// handed over from, another agent:
//   old pane:  "Continued on <agent> · 17:02 · Open"
//   new pane:  "Continued from <agent> · 14 turns · Show brief"
//   failed:    "<agent> couldn't start · Try again from the card"
// It reads the pane's lineage and reloads when `keep-going://changed` names this pane. The state line is always
// `stateText(view)`. Nothing here starts or stops an agent.
import { useEffect, useRef, useState } from 'react';
import {
  keepGoingBrief,
  keepGoingLineage,
  onKeepGoingChanged,
  stateText,
  type HandoverView,
  type KeepGoingLineage,
} from '../../lib/keep-going';
import { BriefSheet } from '../cockpit/BriefSheet';

const LOAD_FAILED = 'Could not load the brief. Try again.';

/**
 * A Rust error arrives as the plain string `CODE: message`. The person sees the message only, in plain words,
 * never the code. An empty message, or any other value, gets the fallback.
 */
export function plainKeepGoingError(err: unknown, fallback: string = LOAD_FAILED): string {
  const raw = (err instanceof Error ? err.message : typeof err === 'string' ? err : '').trim();
  const message = raw.replace(/^[A-Z][A-Z0-9_]*:\s*/, '').trim();
  if (!message) return fallback;
  return message.charAt(0).toUpperCase() + message.slice(1);
}

/** The newer of the two records; `from` = this pane is the new side, `to` = it is the old side. */
export function pickHandover(lineage: KeepGoingLineage | null): HandoverView | null {
  const from = lineage?.from ?? null;
  const to = lineage?.to ?? null;
  if (!from) return to;
  if (!to) return from;
  return from.atMs >= to.atMs ? from : to;
}

function clock(atMs: number): string {
  const d = new Date(atMs);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

const turnsText = (n: number) => (n === 1 ? '1 turn' : `${n} turns`);

export function KeepGoingBanner({ paneId, onOpenPane }: { paneId: string; onOpenPane?: (paneId: string) => void }) {
  const [lineage, setLineage] = useState<KeepGoingLineage | null>(null);
  const [brief, setBrief] = useState<{ agent: string; text: string | null; error: string | null } | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    let unlisten: (() => void) | undefined;
    const load = () => {
      keepGoingLineage(paneId).then(
        (next) => {
          if (alive.current) setLineage(next);
        },
        () => {
          if (alive.current) setLineage(null);
        },
      );
    };
    load();
    onKeepGoingChanged((event) => {
      if (event.paneIds.includes(paneId)) load();
    }).then((stop) => {
      if (alive.current) unlisten = stop;
      else stop();
    });
    return () => {
      alive.current = false;
      unlisten?.();
    };
  }, [paneId]);

  const view = pickHandover(lineage);
  if (!view) return null;

  const isNew = view.toPaneId === paneId;
  const failed = view.state === 'failed';
  const otherPane = isNew ? view.fromPaneId : view.toPaneId;

  const showBrief = () => {
    const agent = view.toLabel;
    setBrief({ agent, text: null, error: null });
    keepGoingBrief(view.id).then(
      (text) => {
        if (alive.current) setBrief({ agent, text, error: null });
      },
      (err: unknown) => {
        if (alive.current) setBrief({ agent, text: null, error: plainKeepGoingError(err) });
      },
    );
  };

  return (
    <>
      <div
        className="ck-chat-note"
        role="status"
        data-keep-going={view.state}
        data-keep-going-side={isNew ? 'new' : 'old'}
        style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}
      >
        <span>{stateText(view)}</span>
        {failed ? (
          <span>· Try again from the card</span>
        ) : (
          <>
            <span aria-hidden="true">·</span>
            <span>{isNew ? turnsText(view.turns) : clock(view.atMs)}</span>
          </>
        )}
        {!failed && isNew && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={showBrief}>
            Show brief
          </button>
        )}
        {!failed && !isNew && onOpenPane && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={() => onOpenPane(otherPane)}>
            Open
          </button>
        )}
      </div>
      {brief && (
        <BriefSheet agent={brief.agent} text={brief.text} error={brief.error} onClose={() => setBrief(null)} />
      )}
    </>
  );
}
