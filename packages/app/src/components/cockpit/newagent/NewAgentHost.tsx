// Connects the New agent page to the cockpit: the agent list, the panes, what needs the person, and the start path.
import { useMemo } from 'react';
import type { CockpitApi } from '../../../hooks/useCockpit';
import { useNeedsYou } from '../../../hooks/useNeedsYou';
import { agentLabel, useAgentRegistry } from '../../../lib/agentRegistry';
import { blockingItems, needsYouPaneIds } from '../../../lib/attention';
import { isCompanionPaneId } from '../companion/model';
import { NewAgentPage, type PaneBrief } from './NewAgentPage';
import { realStartDeps, runStart } from './start';

export function NewAgentHost({ c, onSetUp }: { c: CockpitApi; onSetUp?: () => void }) {
  const listing = useAgentRegistry();
  const needsItems = useNeedsYou(c);
  const waiting = useMemo(() => needsYouPaneIds(blockingItems(needsItems)), [needsItems]);

  const briefs = useMemo<PaneBrief[]>(
    () =>
      c.registry.paneList
        .filter((p) => !isCompanionPaneId(p.info.id))
        .map((p) => ({ id: p.info.id, title: c.names.get(p.info.id) ?? agentLabel(listing, p.info.agent_type), status: p.status })),
    [c.registry.paneList, c.names, listing],
  );
  const selectedCwd = c.selectedId ? c.registry.panes.get(c.selectedId)?.info.cwd : null;

  return (
    <NewAgentPage
      agents={listing.agents}
      panes={briefs}
      needs={briefs.filter((p) => waiting.has(p.id))}
      selectedCwd={selectedCwd}
      onOpenPane={(id) => c.select(id)}
      onSetUp={onSetUp}
      // The agent's own terminal pane, in the folder: the person signs in there (the same login Chat uses).
      onSignIn={(agentId, folder) => c.newAgent(agentId, undefined, false, folder)}
      onStart={(request) => runStart(request, realStartDeps({ newAgent: c.newAgent, select: c.select, notify: c.notify }))}
    />
  );
}
