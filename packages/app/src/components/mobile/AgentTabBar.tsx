// Top bar of the mobile PWA, like terminal tabs: "Agents" plus one tab per agent pane (titled with the
// pane's title), a "+" to open a new agent on the PC and the "⋯" menu. Scrolls sideways when the tabs
// do not fit. Monochrome: the state glyph is the unified dot glyph; the open tab is a filled pill.
import { useEffect, useRef } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import { ALL_TAB, glyphStatus, statusMeta } from '../../lib/mobile-agent-tabs';
import { DotSpinner } from '../cockpit/DotSpinner';

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
      ?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }, [active, panes.length]);

  return (
    <nav className="ph-nav" aria-label="Agents">
      <div ref={stripRef} role="tablist" className="ph-tabs">
        <button
          type="button"
          role="tab"
          data-tab={ALL_TAB}
          aria-selected={active === ALL_TAB}
          onClick={() => onSelect(ALL_TAB)}
          className="ph-tab"
        >
          <span className="ph-tab-in">
            <span className={`ph-conn${bridgeReady ? '' : ' ph-conn--off'}`} role="img" aria-label={bridgeReady ? 'Connected' : 'Connecting'} />
            <span>All</span>
            {attention > 0 && <span className="ph-count">{attention}</span>}
          </span>
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
              className="ph-tab"
            >
              <span className="ph-tab-in">
                <DotSpinner status={glyphStatus(pane.status)} />
                <span className="ph-tab-label">{labels.get(pane.id) ?? pane.agent_type}</span>
              </span>
            </button>
          );
        })}
      </div>
      <button type="button" aria-label="Open a new agent" onClick={onNew} disabled={!bridgeReady} className="ph-iconbtn">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M12 5v14M5 12h14" />
        </svg>
      </button>
      <button type="button" aria-label="Menu" onClick={onMenu} className="ph-iconbtn">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="5.5" cy="12" r="1.2" fill="currentColor" stroke="none" />
          <circle cx="12" cy="12" r="1.2" fill="currentColor" stroke="none" />
          <circle cx="18.5" cy="12" r="1.2" fill="currentColor" stroke="none" />
        </svg>
      </button>
    </nav>
  );
}
