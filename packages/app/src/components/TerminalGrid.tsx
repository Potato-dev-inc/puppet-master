import { useEffect, useMemo, useRef, useState } from 'react';
import type { AgentType } from '@puppet-master/shared';
import { launchMenuEntries, useAgentRegistry } from '../lib/agentRegistry';
import { TerminalPane } from './TerminalPane';
import type { PaneRegistryApi } from '../hooks/usePaneRegistry';
import { isOrchestratorPaneId } from '../lib/orchestrator-panes';
import { detachedWindowSizeFromGrid, openDetachedPaneWindow } from '../lib/detached-pane-window';

interface Props {
  registry: PaneRegistryApi;
  projectPath: string | null;
  onClosePane: (paneId: string) => void;
  detachedPaneIds?: Set<string>;
  /** Panes an agent run opened in the grid: given a wide, tall tile so the TUI keeps a readable PTY size. */
  runPaneIds?: Set<string>;
  onDetachPane?: (paneId: string) => void;
}

const NO_PANES: Set<string> = new Set();

export function TerminalGrid({
  registry,
  projectPath,
  onClosePane,
  detachedPaneIds = NO_PANES,
  runPaneIds = NO_PANES,
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
  const launchEntries = launchMenuEntries(useAgentRegistry());
  const [paneOrder, setPaneOrder] = useState<string[]>([]);
  const [draggingPaneId, setDraggingPaneId] = useState<string | null>(null);
  // Reflow is per pane, never per grid. Only a pane that came back from its own window has to be
  // re-fitted at the grid size (it mounted while the PTY was still sized to its old home); every
  // other pane is left alone when a neighbour opens, closes or changes status, because each refit
  // that changes a cell's size resizes the PTY and makes an inline TUI reprint its whole screen.
  // A cell whose size really changes (drag-to-resize, window resize) is refitted by the terminal's
  // own ResizeObserver, not from here.
  const [reattach, setReattach] = useState<Record<string, number>>({});
  // Second-phase reflow after the visible settle animation finishes (reattach path).
  const [postSettle, setPostSettle] = useState<Record<string, number>>({});
  // Live cell <div>s, keyed by pane id, so we can animate a visible
  // shrink-and-restore when a pane returns to the grid.
  const cellRefs = useRef(new Map<string, HTMLDivElement>());
  const previousDetached = useRef(detachedPaneIds);
  const settledGeneration = useRef<Record<string, number>>({});

  // Panes that left `detachedPaneIds` since the last render: they came back from a pop-out window.
  useEffect(() => {
    const before = previousDetached.current;
    previousDetached.current = detachedPaneIds;
    if (before === detachedPaneIds) return;
    const returned = [...before].filter((id) => !detachedPaneIds.has(id) && !isOrchestratorPaneId(id));
    if (returned.length === 0) return;
    setReattach((prev) => {
      const next = { ...prev };
      for (const id of returned) next[id] = (prev[id] ?? 0) + 1;
      return next;
    });
  }, [detachedPaneIds]);

  // Visible "settle" animation for a pane coming back from an individual/detached window: briefly
  // shrink its cell and let it spring back. The transition makes the resize visible, and each
  // animated frame trips the terminal's ResizeObserver so it re-fits as the box moves, the same
  // reliable path as dragging the resize handle. Only the returned panes' cells are touched.
  useEffect(() => {
    const due = Object.keys(reattach).filter((id) => reattach[id] !== settledGeneration.current[id]);
    if (due.length === 0) return;
    for (const id of due) settledGeneration.current[id] = reattach[id];

    const rafs: number[] = [];
    const timeouts: number[] = [];
    const animated: string[] = [];

    for (const id of due) {
      const cell = cellRefs.current.get(id);
      if (!cell || !cell.isConnected) continue;
      animated.push(id);
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
      timeouts.push(window.setTimeout(() => {
        cell.classList.remove('pm-pane-settle');
      }, 420));
    }

    if (animated.length > 0) {
      timeouts.push(window.setTimeout(() => {
        setPostSettle((prev) => {
          const next = { ...prev };
          for (const id of animated) next[id] = (prev[id] ?? 0) + 1;
          return next;
        });
      }, 420));
    }

    return () => {
      for (const id of rafs) cancelAnimationFrame(id);
      for (const id of timeouts) clearTimeout(id);
    };
  }, [reattach]);

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

  // Changes only for a pane that came back from its own window (and once more when its settle ends).
  const reflowKeyFor = (paneId: string) => `${reattach[paneId] ?? 0}:${postSettle[paneId] ?? 0}`;

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
            className={`resize-both min-w-[320px] min-h-[240px] overflow-hidden rounded border border-pm-border/40 ${
              runPaneIds.has(pane.info.id) ? 'xl:col-span-2 h-[580px]' : 'h-[280px]'
            } ${
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
              reflowKey={reflowKeyFor(pane.info.id)}
              redrawKey={reflowKeyFor(pane.info.id)}
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
            <div className="absolute top-full left-0 mt-1 z-10 w-64 rounded-md border border-pm-border bg-pm-panel shadow-lg">
              {launchEntries.map((p) => (
                <button
                  key={p.type}
                  onClick={() => addPane(p.type)}
                  disabled={p.disabled}
                  title={p.disabledReason}
                  className="w-full text-left px-3 py-2 text-xs hover:bg-pm-border/40 disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-transparent"
                >
                  {p.label}
                  {p.experimental && <span className="ml-1 text-[10px] text-pm-warn">experimental</span>}
                  {p.disabledReason && (
                    <span className="block text-[10px] text-pm-muted">{p.disabledReason}</span>
                  )}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </main>
  );
}
