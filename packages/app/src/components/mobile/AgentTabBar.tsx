// Top tab strip of the mobile PWA, like terminal tabs: "All" plus one tab per agent pane, a "+" to
// open a new agent on the PC and the "⋯" menu. Scrolls sideways when the tabs do not fit.
import { useEffect, useRef } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import { ALL_TAB, statusMeta } from '../../lib/mobile-agent-tabs';

export function AgentTabBar({
  panes,
  labels,
  active,
  bridgeReady,
  onSelect,
  onNew,
  onMenu,
}: {
  panes: PaneInfo[];
  labels: Map<string, string>;
  active: string;
  bridgeReady: boolean;
  onSelect: (tab: string) => void;
  onNew: () => void;
  onMenu: () => void;
}) {
  const stripRef = useRef<HTMLDivElement>(null);
  const attention = panes.filter((pane) => statusMeta(pane.status).attention).length;

  useEffect(() => {
    stripRef.current
      ?.querySelector<HTMLElement>(`[data-tab="${CSS.escape(active)}"]`)
      ?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [active, panes.length]);

  const tabClass = (selected: boolean) =>
    `relative flex h-10 shrink-0 items-center gap-1.5 rounded-t-md px-3 text-xs ${
      selected
        ? 'bg-pm-panel text-zinc-100 before:absolute before:inset-x-2 before:top-0 before:h-0.5 before:rounded-full before:bg-pm-accent'
        : 'text-pm-muted active:bg-pm-raised'
    }`;

  return (
    <nav
      className="flex shrink-0 items-end gap-1 border-b border-pm-border bg-pm-bg pl-1 pr-1 pt-[env(safe-area-inset-top,0px)]"
      aria-label="Agents"
    >
      <div ref={stripRef} role="tablist" className="flex min-w-0 flex-1 items-end gap-0.5 overflow-x-auto no-scrollbar">
        <button
          type="button"
          role="tab"
          data-tab={ALL_TAB}
          aria-selected={active === ALL_TAB}
          onClick={() => onSelect(ALL_TAB)}
          className={tabClass(active === ALL_TAB)}
        >
          <span className={`h-2 w-2 rounded-full ${bridgeReady ? 'bg-pm-ok' : 'bg-pm-warn'}`} />
          <span className="font-medium">All</span>
          {attention > 0 && (
            <span className="rounded-full bg-pm-warn px-1.5 text-[10px] font-semibold leading-4 text-pm-bg">{attention}</span>
          )}
        </button>
        {panes.map((pane) => {
          const meta = statusMeta(pane.status);
          return (
            <button
              key={pane.id}
              type="button"
              role="tab"
              data-tab={pane.id}
              aria-selected={active === pane.id}
              title={`${labels.get(pane.id)} · ${meta.label}`}
              onClick={() => onSelect(pane.id)}
              className={tabClass(active === pane.id)}
            >
              <span className={`h-2 w-2 shrink-0 rounded-full ${meta.dot} ${meta.pulse ? 'animate-pulse' : ''}`} />
              <span className="max-w-[9rem] truncate font-mono">{labels.get(pane.id) ?? pane.agent_type}</span>
            </button>
          );
        })}
      </div>
      <button
        type="button"
        aria-label="Open a new agent"
        onClick={onNew}
        disabled={!bridgeReady}
        className="mb-1 flex h-8 w-8 shrink-0 items-center justify-center rounded text-lg text-pm-muted active:bg-pm-raised disabled:opacity-40"
      >
        +
      </button>
      <button
        type="button"
        aria-label="Menu"
        onClick={onMenu}
        className="mb-1 flex h-8 w-8 shrink-0 items-center justify-center rounded text-lg text-pm-muted active:bg-pm-raised"
      >
        ⋯
      </button>
    </nav>
  );
}
