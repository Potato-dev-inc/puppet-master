// The open agent's own approval, docked above its terminal (design 2.1): one card at a time, and
// under it how many other things wait and the next one. The terminal dims behind it. A card of another
// pane is never swapped in by itself: "Next" is a tap.
import { useCallback, useEffect, useState } from 'react';
import type { AttentionSnapshot } from '../../lib/attention';
import { COPY } from '../../lib/attention-copy';
import type { BridgeClient } from '../../lib/bridge-types';
import { dockFor } from '../../lib/needs-you-queue';
import { ApprovalCard } from './ApprovalCard';
import '../../styles/mobile-approval.css';

export function DockedCard({
  snapshot,
  paneId,
  bridge,
  canAnswer,
  onOpenPane,
  onOpenItem,
  autoFocus = false,
}: {
  snapshot: AttentionSnapshot;
  paneId: string;
  bridge: BridgeClient;
  canAnswer: boolean;
  onOpenPane: (paneId: string) => void;
  /** Open an item that has no pane of its own (a question, a consent request). */
  onOpenItem: (itemId: string) => void;
  autoFocus?: boolean;
}) {
  const [now, setNow] = useState(() => Date.now());
  const [own, setOwn] = useState<ReadonlyMap<string, number>>(() => new Map());
  const markOwn = useCallback((id: string) => setOwn((prev) => (prev.has(id) ? prev : new Map(prev).set(id, Date.now()))), []);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const { current, more, next } = dockFor(snapshot, paneId, own, now);
  if (!current && more === 0) return null;
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
        />
      )}
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
