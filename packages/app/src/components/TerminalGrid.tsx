import { useEffect, useMemo, useRef, useState } from 'react';
import { listLaunchPresets, type AgentType } from '@puppet-master/shared';
import { TerminalPane } from './TerminalPane';
import type { PaneRegistryApi } from '../hooks/usePaneRegistry';
import { isOrchestratorPaneId } from '../lib/orchestrator-panes';
import { detachedWindowSizeFromGrid, openDetachedPaneWindow } from '../lib/detached-pane-window';

interface Props {
  registry: PaneRegistryApi;
  projectPath: string | null;
  onClosePane: (paneId: string) => void;
  detachedPaneIds?: Set<string>;
  onDetachPane?: (paneId: string) => void;
}

export function TerminalGrid({
  registry,
  projectPath,
  onClosePane,
  detachedPaneIds = new Set(),
  onDetachPane,
}: Props) {
  const { paneList, spawnPane, replacePaneAgent } = registry;
  const workerPanes = useMemo(
    () => paneList.filter((pane) => (
      !isOrchestratorPaneId(pane.info.id) &&
      !detachedPaneIds.has(pane.info.id)
    )),
    [detachedPaneIds, paneList],
  );
  const workerPaneIds = useMemo(
    () => workerPanes.map((pane) => pane.info.id).join('\0'),
    [workerPanes],
  );
  const [showAdd, setShowAdd] = useState(false);
  const [paneOrder, setPaneOrder] = useState<string[]>([]);
  const [draggingPaneId, setDraggingPaneId] = useState<string | null>(null);
  // Bumped whenever the set of panes in the grid changes (e.g. a pane returning
  // from a detached/individual window). Forwarded to each pane as a reflow key
  // so the terminals do a tiny re-fit and repaint perfectly at the grid size.
  const [reflowGeneration, setReflowGeneration] = useState(0);
  // Second-phase reflow after the visible settle animation finishes (reattach path).
  const [postSettleReflowGeneration, setPostSettleReflowGeneration] = useState(0);
  // Live cell <div>s, keyed by pane id, so we can animate a visible
  // shrink-and-restore when panes return to the grid.
  const cellRefs = useRef(new Map<string, HTMLDivElement>());
  const isFirstRender = useRef(true);

  useEffect(() => {
    setReflowGeneration((n) => n + 1);
  }, [workerPaneIds]);

  // Visible "settle" animation: when the grid's pane set changes (most notably a
  // pane coming back from an individual/detached window), briefly shrink each
  // cell and let it spring back. The transition makes the resize visible, and
  // each animated frame trips the terminal's ResizeObserver so it re-fits as the
  // box moves — the same reliable path as dragging the resize handle.
  useEffect(() => {
    // Skip the very first mount; only animate on subsequent membership changes.
    if (isFirstRender.current) {
      isFirstRender.current = false;
      return;
    }

    const cells = Array.from(cellRefs.current.values());
    const rafs: number[] = [];
    const timeouts: number[] = [];

    for (const cell of cells) {
      if (!cell.isConnected) continue;
      cell.classList.add('pm-pane-settle');
      // Snap to the shrunk size, then release it after it's been painted so the
      // CSS transition animates the height back to natural. A double rAF ensures
      // the shrunk frame actually renders before we remove it (a single frame is
      // often too fast and the browser collapses both into one paint, skipping
      // the transition).
      cell.classList.add('pm-pane-settle--shrink');
      const release = requestAnimationFrame(() => {
        const release2 = requestAnimationFrame(() => {
          cell.classList.remove('pm-pane-settle--shrink');
        });
        rafs.push(release2);
      });
      rafs.push(release);
      // Drop the transition wrapper once the animation has finished.
      const cleanup = window.setTimeout(() => {
        cell.classList.remove('pm-pane-settle');
      }, 420);
      timeouts.push(cleanup);
    }

    if (cells.some((cell) => cell.isConnected)) {
      const postSettle = window.setTimeout(() => {
        setPostSettleReflowGeneration((n) => n + 1);
      }, 420);
      timeouts.push(postSettle);
    }

    return () => {
      for (const id of rafs) cancelAnimationFrame(id);
      for (const id of timeouts) clearTimeout(id);
    };
  }, [reflowGeneration]);

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

  const popOutPane = async (paneId: string, size?: { width: number; height: number }) => {
    const pane = registry.panes.get(paneId);
    const label = pane ? `${pane.info.agent_type} · ${pane.info.id.slice(0, 8)}` : `Pane ${paneId}`;
    await openDetachedPaneWindow(
      paneId,
      label,
      size ?? detachedWindowSizeFromGrid(pane?.info.cols, pane?.info.rows),
    );
    onDetachPane?.(paneId);
  };

  return (
    <main className="flex-1 min-h-0 overflow-auto p-3 bg-pm-bg">
      <div className="grid grid-cols-1 xl:grid-cols-2 gap-3 auto-rows-[minmax(280px,auto)]">
        {orderedPanes.map((pane) => (
          <div
            key={`${pane.info.id}-${pane.info.created_at}`}
            ref={(el) => {
              if (el) {
                cellRefs.current.set(pane.info.id, el);
              } else {
                cellRefs.current.delete(pane.info.id);
              }
            }}
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
              onPopOut={(paneId, size) => void popOutPane(paneId, size)}
              onSwitchAgent={switchAgent}
              syncPTYResize={!detachedPaneIds.has(pane.info.id)}
              reflowKey={`${reflowGeneration}:${postSettleReflowGeneration}`}
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
              {listLaunchPresets().map((p) => (
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
