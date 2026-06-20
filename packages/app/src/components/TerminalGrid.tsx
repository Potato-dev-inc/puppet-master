import { useEffect, useMemo, useState } from 'react';
import { listPresets, type AgentType } from '@puppet-master/shared';
import { TerminalPane } from './TerminalPane';
import type { PaneRegistryApi } from '../hooks/usePaneRegistry';
import { isOrchestratorPaneId } from '../lib/orchestrator-panes';

interface Props {
  registry: PaneRegistryApi;
  projectPath: string | null;
  onClosePane: (paneId: string) => void;
}

export function TerminalGrid({ registry, projectPath, onClosePane }: Props) {
  const { paneList, spawnPane, replacePaneAgent } = registry;
  const workerPanes = useMemo(
    () => paneList.filter((pane) => !isOrchestratorPaneId(pane.info.id)),
    [paneList],
  );
  const workerPaneIds = useMemo(
    () => workerPanes.map((pane) => pane.info.id).join('\0'),
    [workerPanes],
  );
  const [showAdd, setShowAdd] = useState(false);
  const [paneOrder, setPaneOrder] = useState<string[]>([]);
  const [draggingPaneId, setDraggingPaneId] = useState<string | null>(null);

  useEffect(() => {
    const liveIds = workerPaneIds ? workerPaneIds.split('\0') : [];
    setPaneOrder((prev) => {
      const retained = prev.filter((id) => liveIds.includes(id));
      const added = liveIds.filter((id) => !retained.includes(id));
      const next = [...retained, ...added];
      if (next.length === prev.length && next.every((id, index) => id === prev[index])) {
        return prev;
      }
      return next;
    });
  }, [workerPaneIds]);

  const orderedPanes = useMemo(() => {
    const byId = new Map(workerPanes.map((pane) => [pane.info.id, pane]));
    const ordered = paneOrder.flatMap((id) => {
      const pane = byId.get(id);
      return pane ? [pane] : [];
    });
    const missing = workerPanes.filter((pane) => !paneOrder.includes(pane.info.id));
    return [...ordered, ...missing];
  }, [workerPanes, paneOrder]);

  const addPane = async (agent: AgentType) => {
    setShowAdd(false);
    await spawnPane({ agent_type: agent, cwd: projectPath ?? undefined });
  };

  const switchAgent = async (paneId: string, agent: AgentType) => {
    await replacePaneAgent(paneId, agent, projectPath ?? undefined);
  };

  return (
    <main className="flex-1 min-h-0 overflow-auto p-3 bg-pm-bg">
      <div className="grid grid-cols-1 xl:grid-cols-2 gap-3 auto-rows-[minmax(280px,auto)]">
        {orderedPanes.map((pane) => (
          <div
            key={`${pane.info.id}-${pane.info.created_at}`}
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => {
              event.preventDefault();
              if (!draggingPaneId || draggingPaneId === pane.info.id) return;
              setPaneOrder((prev) => {
                const withoutDragged = prev.filter((id) => id !== draggingPaneId);
                const targetIndex = withoutDragged.indexOf(pane.info.id);
                if (targetIndex < 0) return prev;
                return [
                  ...withoutDragged.slice(0, targetIndex),
                  draggingPaneId,
                  ...withoutDragged.slice(targetIndex),
                ];
              });
              setDraggingPaneId(null);
            }}
            onDragEnd={() => setDraggingPaneId(null)}
            className={`resize-both min-w-[320px] min-h-[240px] h-[280px] overflow-hidden rounded border border-pm-border/40 ${
              draggingPaneId === pane.info.id ? 'opacity-60' : ''
            }`}
            title="Drag the header grip to reorder, or drag the lower-right corner to resize this pane"
          >
            <TerminalPane
              pane={pane}
              subscribePaneData={registry.subscribePaneData}
              onClose={onClosePane}
              onSwitchAgent={switchAgent}
              dragHandleProps={{
                draggable: true,
                onDragStart: () => setDraggingPaneId(pane.info.id),
                onDragEnd: () => setDraggingPaneId(null),
              }}
            />
          </div>
        ))}
        <div className="relative">
          <button
            onClick={() => setShowAdd((s) => !s)}
            className="w-full h-full rounded-lg border border-dashed border-pm-border text-pm-muted hover:bg-pm-panel/40 hover:text-zinc-200 flex flex-col items-center justify-center text-sm gap-0.5"
          >
            <span className="text-lg leading-none">+</span>
            <span>New session</span>
          </button>
          {showAdd && (
            <div className="absolute top-full left-0 mt-1 z-10 w-48 rounded-md border border-pm-border bg-pm-panel shadow-lg">
              {listPresets().map((p) => (
                <button
                  key={p.type}
                  onClick={() => addPane(p.type)}
                  className="w-full text-left px-3 py-2 text-xs hover:bg-pm-border/40"
                >
                  {p.label}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </main>
  );
}
