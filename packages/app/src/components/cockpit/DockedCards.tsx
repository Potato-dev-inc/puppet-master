// What waits on the person in a pane, docked: the Terminal tab floats these cards over the terminal, the Chat
// tab shows them under the conversation. The same cards in the same order; "N more ›" cycles through them.
import type { CockpitApi } from '../../hooks/useCockpit';
import type { DockItem } from './pane-chrome';
import { PaneApprovalCard } from './PaneApprovalCard';
import { PaneStuckCard } from './StuckCard';

export function DockedCards({
  c,
  paneId,
  dock,
  dockNow,
  onMore,
}: {
  c: CockpitApi;
  paneId: string;
  dock: DockItem[];
  dockNow: DockItem;
  onMore: () => void;
}) {
  const prompt = c.prompts[paneId];
  return (
    <div className="ck-dock">
      {dockNow === 'stuck' && <PaneStuckCard c={c} paneId={paneId} />}
      {/* The Inbox's card, docked: same words, same order, same keyboard rules; no "Open terminal" here. */}
      {dockNow === 'ask' && prompt && (
        <div className="ib-dock" role="group" aria-label="Agent question">
          <PaneApprovalCard c={c} paneId={paneId} />
        </div>
      )}
      {dock.length > 1 && (
        <button type="button" className="ck-dock-more" onClick={onMore}>
          {dock.length - 1} more ›
        </button>
      )}
    </div>
  );
}
