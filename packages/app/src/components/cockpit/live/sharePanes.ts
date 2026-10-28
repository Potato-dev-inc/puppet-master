// The panes the Go live sheet lists: every pane in the cockpit, helpers under their parent, with the ones that
// cannot be shared greyed (a companion's chat is private; a plain shell has no chat).
import type { AgentRegistryListing } from '../../../lib/agentRegistry';
import { isCompanionPaneId } from '../companion/model';
import { paneHeading } from '../titles';
import type { SharePane } from './GoLiveSheet';

interface PaneLike {
  info: { id: string; agent_type?: string | null; title?: string | null; parent_pane_id?: string | null };
}

export function sharePanes(paneList: PaneLike[], names: Map<string, string>, listing: AgentRegistryListing): SharePane[] {
  return paneList.map((p) => {
    const { id } = p.info;
    const agent = p.info.agent_type ?? '';
    const kind = listing.agents.find((a) => a.id === agent)?.kind;
    return {
      id,
      label: paneHeading(p.info, names.get(id) ?? agent).primary,
      parentId: p.info.parent_pane_id ?? null,
      blocked: isCompanionPaneId(id) ? 'companion' : kind === 'shell' ? 'no-chat' : null,
    };
  });
}
