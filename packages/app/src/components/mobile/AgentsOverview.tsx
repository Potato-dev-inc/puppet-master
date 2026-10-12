// "All" tab of the mobile PWA: one card per agent pane with its status, project, last activity and
// the last lines on its screen. Tapping a card opens that agent's tab.
import { useEffect, useState } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import { ago, previewLines, projectName, statusMeta } from '../../lib/mobile-agent-tabs';

const PREVIEW_POLL_MS = 4000;

export function AgentsOverview({
  bridge,
  bridgeReady,
  panes,
  labels,
  onOpen,
  onNew,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  panes: PaneInfo[];
  labels: Map<string, string>;
  onOpen: (paneId: string) => void;
  onNew: () => void;
}) {
  const [previews, setPreviews] = useState<Record<string, string[]>>({});
  const [now, setNow] = useState(() => Date.now());
  const ids = panes.map((pane) => pane.id).join(',');

  useEffect(() => {
    if (!bridgeReady || !ids) return;
    let live = true;
    const tick = async () => {
      setNow(Date.now());
      const entries = await Promise.all(
        ids.split(',').map(async (id) => {
          try {
            return [id, previewLines(await bridge.readBuffer(id, 60))] as const;
          } catch {
            return [id, null] as const;
          }
        }),
      );
      if (!live) return;
      setPreviews((prev) => {
        const next: Record<string, string[]> = {};
        for (const [id, lines] of entries) next[id] = lines ?? prev[id] ?? [];
        return next;
      });
    };
    void tick();
    const timer = window.setInterval(() => void tick(), PREVIEW_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [bridge, bridgeReady, ids]);

  if (!bridgeReady) {
    return <div className="py-10 text-center text-xs text-pm-muted">Connecting to your PC…</div>;
  }

  if (panes.length === 0) {
    return (
      <div className="flex flex-col items-center gap-3 px-6 py-14 text-center">
        <div className="text-sm text-zinc-200">No agents running on your PC</div>
        <div className="text-xs text-pm-muted">
          Agents you open on the PC, or that another agent starts through Puppet Master, show up here as tabs.
        </div>
        <button
          type="button"
          onClick={onNew}
          className="mt-2 rounded border border-pm-border px-3 py-2 text-xs text-zinc-100 active:bg-pm-raised"
        >
          Open an agent
        </button>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto overscroll-contain p-2 pb-[calc(0.5rem+env(safe-area-inset-bottom,0px))]">
      <ul className="flex flex-col gap-2">
        {panes.map((pane) => {
          const meta = statusMeta(pane.status);
          const lines = previews[pane.id] ?? [];
          const last = ago(pane.last_output_at, now);
          return (
            <li key={pane.id}>
              <button
                type="button"
                onClick={() => onOpen(pane.id)}
                className={`w-full rounded-lg border bg-pm-panel p-3 text-left active:bg-pm-raised ${
                  meta.attention ? 'border-pm-warn/60' : 'border-pm-border'
                }`}
              >
                <div className="flex items-center gap-2 text-xs">
                  <span className={`h-2 w-2 shrink-0 rounded-full ${meta.dot} ${meta.pulse ? 'animate-pulse' : ''}`} />
                  <span className="truncate font-mono text-zinc-100">{labels.get(pane.id) ?? pane.agent_type}</span>
                  <span className="truncate text-pm-muted">{projectName(pane.cwd)}</span>
                  <span className={`ml-auto shrink-0 ${meta.attention ? 'text-pm-warn' : 'text-pm-muted'}`}>{meta.label}</span>
                </div>
                <div className="mt-2 min-h-[2.5rem] rounded bg-pm-bg/70 px-2 py-1.5 font-mono text-[11px] leading-snug text-zinc-300">
                  {lines.length > 0 ? (
                    lines.map((line, i) => (
                      <div key={i} className="truncate">
                        {line}
                      </div>
                    ))
                  ) : (
                    <div className="text-pm-muted">No output yet</div>
                  )}
                </div>
                {last && <div className="mt-1.5 text-[10px] text-pm-muted">last output {last}</div>}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
