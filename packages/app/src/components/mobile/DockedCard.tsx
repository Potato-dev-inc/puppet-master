// The open agent's own approval, docked above its terminal (design 2.1): one card at a time, and
// under it how many other things wait and the next one. The terminal dims behind it. A card of another
// pane is never swapped in by itself: "Next" is a tap.
import { useCallback, useEffect, useState } from 'react';
import type { AttentionSnapshot } from '../../lib/attention';
import { COPY } from '../../lib/attention-copy';
import type { BridgeClient } from '../../lib/bridge-types';
import { helperItems } from '../../lib/helper-questions';
import { PROMPT_KINDS, dockFor } from '../../lib/needs-you-queue';
import { ApprovalCard } from './ApprovalCard';
import '../../styles/mobile-approval.css';

export function DockedCard({
  snapshot,
  paneId,
  bridge,
  canAnswer,
  onOpenPane,
  onOpenItem,
  onRefresh,
  autoFocus = false,
  helperPaneIds = [],
}: {
  snapshot: AttentionSnapshot;
  paneId: string;
  bridge: BridgeClient;
  canAnswer: boolean;
  onOpenPane: (paneId: string) => void;
  /** Open an item that has no pane of its own (a question, a consent request). */
  onOpenItem: (itemId: string) => void;
  /** An answer came back stale: read the PC's list again. */
  onRefresh?: () => void;
  autoFocus?: boolean;
  /** The chat of a pane that started helpers: the helpers that ask, whose cards sit under its own. */
  helperPaneIds?: string[];
}) {
  const [now, setNow] = useState(() => Date.now());
  const [own, setOwn] = useState<ReadonlyMap<string, number>>(() => new Map());
  const markOwn = useCallback((id: string) => setOwn((prev) => (prev.has(id) ? prev : new Map(prev).set(id, Date.now()))), []);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const helperCards = helperItems(snapshot.items, helperPaneIds, (item) => PROMPT_KINDS.includes(item.kind));
  const { current, more, next } = dockFor(snapshot, paneId, own, now, new Set(helperCards.map((i) => i.id)));
  if (!current && more === 0 && helperCards.length === 0) return null;
  const nextHeading = next?.pane?.heading ?? next?.title;
  return (
    <div className="ph-dock" data-testid="docked-card">
      {current && (
        <ApprovalCard
          key={current.id}
          item={current}
          bridge={bridge}
          canAnswer={canAnswer}
          variant="docked"
          autoFocus={autoFocus}
          onOpenPane={onOpenPane}
          onAnswered={() => markOwn(current.id)}
          onRefresh={onRefresh}
        />
      )}
      {helperCards.map((item) => (
        <div key={item.id} className="ph-dock-helper" data-testid="helper-question">
          <p className="ph-dock-helper-lead">{item.pane?.heading ?? 'A helper'} helper asks</p>
          <ApprovalCard item={item} bridge={bridge} canAnswer={canAnswer} variant="docked" onOpenPane={onOpenPane} onRefresh={onRefresh} />
          {item.pane && (
            <button type="button" className="ph-btn ph-btn--plain" data-role="open-helper" onClick={() => onOpenPane(item.pane!.id)}>
              Open helper
            </button>
          )}
        </div>
      ))}
      {more > 0 && next && (
        <p className="ph-queue-line">
          <span>{COPY.queue(more)}</span>
          <button type="button" onClick={() => (next.pane ? onOpenPane(next.pane.id) : onOpenItem(next.id))}>
            Next: {nextHeading} ›
          </button>
        </p>
      )}
    </div>
  );
}
