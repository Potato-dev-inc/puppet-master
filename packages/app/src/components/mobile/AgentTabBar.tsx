// Top bar of the mobile PWA, like terminal tabs: one tab per companion (its face, first), "Agents", one tab per agent
// pane (titled with the pane's title), a "+" to open a new agent on the PC and the "⋯" menu. Scrolls sideways when the
// tabs do not fit. Monochrome: the state glyph is the unified dot glyph; the open tab is a filled pill. A companion's
// own pane (`companion-<id>`) has no tab of its own: its companion tab stands for it.
import { useEffect, useRef } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import type { CompanionView } from '../../lib/companion';
import { stateWord } from '../../lib/companion';
import { ALL_TAB, companionTab, glyphStatus, isCompanionPane, statusMeta } from '../../lib/mobile-agent-tabs';
import { DotSpinner } from '../cockpit/DotSpinner';
import { Avatar } from '../companion/Avatar';

/** The companion's face on its tab (SPEC-1028 section 7). */
export const TAB_FACE_PX = 22;

export function AgentTabBar({
  panes,
  labels,
  active,
  bridgeReady,
  onSelect,
  onNew,
  onMenu,
  needsYou,
  companions = [],
}: {
  panes: PaneInfo[];
  labels: Map<string, string>;
  active: string;
  bridgeReady: boolean;
  onSelect: (tab: string) => void;
  onNew: () => void;
  onMenu: () => void;
  /** The PC's one number (open blocking items). Without it (an older PC) the bar counts waiting and failed panes. */
  needsYou?: number;
  /** The companions, shown first with their faces. */
  companions?: CompanionView[];
}) {
  const stripRef = useRef<HTMLDivElement>(null);
  const agentPanes = panes.filter((pane) => !isCompanionPane(pane.id));
  const attention = needsYou ?? agentPanes.filter((pane) => statusMeta(pane.status).attention).length;

  useEffect(() => {
    stripRef.current
      ?.querySelector<HTMLElement>(`[data-tab="${CSS.escape(active)}"]`)
      ?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }, [active, panes.length, companions.length]);

  return (
    <nav className="ph-nav" aria-label="Agents">
      <div ref={stripRef} role="tablist" className="ph-tabs">
        {companions.map((companion) => (
          <button
            key={companion.id}
            type="button"
            role="tab"
            data-tab={companionTab(companion.id)}
            aria-selected={active === companionTab(companion.id)}
            title={`${companion.name} · ${stateWord(companion.state)}`}
            onClick={() => onSelect(companionTab(companion.id))}
            className="ph-tab"
          >
            <span className="ph-tab-in">
              <Avatar id={companion.avatar} size={TAB_FACE_PX} state={companion.state} name={companion.name} />
              <span className="ph-tab-label">{companion.name}</span>
            </span>
          </button>
        ))}
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
        {agentPanes.map((pane) => {
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
