import { useContext, useEffect, useRef, useState } from 'react';
import { NewAgentMenu, type ProjectChoice } from './NewAgentMenu';
import { projectOfCwd, readCopyPreference, supportsCopies, writeCopyPreference } from '../../lib/ship-model';
import type { CockpitApi } from '../../hooks/useCockpit';
import { useNeedsYou } from '../../hooks/useNeedsYou';
import { blockingItems, needsYouPaneIds } from '../../lib/attention';
import { projectName } from '../../lib/mobile-agent-tabs';
import { activityText } from '../../lib/pane-activity';
import { mainPreview, previewLabel } from '../../lib/pane-preview';
import '../../styles/live-preview.css';
import { DotSpinner } from './DotSpinner';
import { IconGlobe, IconInbox, IconMoon, IconNote, IconPlus, IconSearch, IconSettings, IconSun, IconTerminal } from './Icons';
import { formatElapsed, groupPanes, spinnerStatus } from './model';
import { multiLoginAgents, paneHeading, paneSubline } from './titles';
import { LastTurnContext } from './away-context';
import { SegTabs } from './SegTabs';

export function Sidebar({
  c,
  onOpenPalette,
  onOpenDoctor,
  onOpenMemory,
}: {
  c: CockpitApi;
  onOpenPalette: () => void;
  onOpenDoctor?: () => void;
  onOpenMemory?: () => void;
}) {
  const list = c.registry.paneList;
  // The same list the Inbox badge and the Doto counter use, so the three numbers always agree.
  const needs = useNeedsYou(c);
  const questionPanes = needsYouPaneIds(needs);
  const groups = groupPanes(list, questionPanes);
  const waiting = blockingItems(needs).length;
  const multiLogin = multiLoginAgents(list.map((p) => p.info));
  const lastTurn = useContext(LastTurnContext);
  const [menuOpen, setMenuOpen] = useState(false);
  // The folder step: an explicit choice wins, else the selected pane's project.
  const [chosen, setChosen] = useState<ProjectChoice | null>(null);
  const selectedCwd = c.selectedId ? c.registry.panes.get(c.selectedId)?.info.cwd : undefined;
  const project: ProjectChoice | null = chosen ?? projectOfCwd(selectedCwd);
  const projectKey = project?.path ?? null;
  const [isolated, setIsolated] = useState(() => readCopyPreference(projectKey));
  useEffect(() => setIsolated(readCopyPreference(projectKey)), [projectKey]);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const open = () => setMenuOpen(true);
    window.addEventListener('pm:open-new-menu', open);
    return () => window.removeEventListener('pm:open-new-menu', open);
  }, []);

  useEffect(() => {
    if (!menuOpen) return;
    const away = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setMenuOpen(false);
    window.addEventListener('mousedown', away);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('keydown', esc);
    };
  }, [menuOpen]);

  const section = (title: string, items: typeof list, count?: number) =>
    items.length === 0 ? null : (
      <div className="ck-group" key={title}>
        <div className="ck-caption">
          {title}
          {count ? <span className="ck-caption-count">{count}</span> : null}
        </div>
        {items.map((p) => {
          const id = p.info.id;
          const since = c.statusSince.get(id);
          const active = c.selectedId === id && !c.spawn;
          const heading = paneHeading(p.info, c.names.get(id) ?? p.info.agent_type);
          const subline = paneSubline(
            p.info,
            c.names.get(id) ?? p.info.agent_type,
            activityText(p.status, c.activities?.[id]) ??
              (p.status === 'error' ? `Error · ${projectName(p.info.cwd)}` : projectName(p.info.cwd)),
            multiLogin,
          );
          return (
            <button
              key={id}
              type="button"
              className={`ck-row${active ? ' is-active' : ''}${p.status === 'waiting_input' || questionPanes.has(id) ? ' is-needs' : ''}`}
              onClick={() => c.select(id)}
              title={lastTurn.get(id)?.sentence}
              aria-current={active ? 'true' : undefined}
            >
              <span className="ck-row-glyph">
                <DotSpinner status={spinnerStatus(p.status)} />
              </span>
              <span className="ck-row-main" title={[...new Set([heading.primary, subline, projectName(p.info.cwd)])].join(' · ')}>
                <span className="ck-row-title">{heading.primary}</span>
                <span className="ck-row-sub">{subline}</span>
              </span>
              {mainPreview(c.previews, id) && (
                <span className="ck-row-preview" title={`Preview ${previewLabel(mainPreview(c.previews, id)!)}`} role="img" aria-label="Has a preview">
                  <IconGlobe />
                </span>
              )}
              <span className="ck-row-time">{since ? formatElapsed(c.now - since) : ''}</span>
            </button>
          );
        })}
      </div>
    );

  return (
    <aside className="ck-side" aria-label="Agents">
      <div className="ck-side-head">
        <span className="ck-wordmark">Puppet Master</span>
        <button type="button" className="ck-icon-btn" onClick={onOpenPalette} title="Search (Ctrl+K)" aria-label="Search">
          <IconSearch />
        </button>
        <div className="ck-menu-wrap" ref={menuRef}>
          <button
            type="button"
            className="ck-icon-btn"
            onClick={() => setMenuOpen((v) => !v)}
            title="New agent"
            aria-label="New agent"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
          >
            <IconPlus />
          </button>
          {menuOpen && (
            <div className="ck-menu ck-menu--left ck-menu--wide" role="menu">
              <NewAgentMenu
                entries={c.launchEntries}
                readyAgents={c.readyAgents}
                onStart={(agent, account) => {
                  const copy = isolated && supportsCopies(project?.state);
                  void (project ? c.newAgent(agent, account, copy, project.path) : c.newAgent(agent, account, copy));
                }}
                onClose={() => setMenuOpen(false)}
                project={project}
                onProject={setChosen}
                isolated={isolated}
                onIsolated={(on) => {
                  setIsolated(on);
                  writeCopyPreference(projectKey, on);
                }}
              />
            </div>
          )}
        </div>
      </div>

      <nav className="ck-side-scroll">
        <button
          type="button"
          className={`ck-row ck-row--flat${c.selectedId === null && !c.spawn ? ' is-active' : ''}`}
          onClick={() => c.select(null)}
        >
          <span className="ck-row-glyph ck-row-glyph--icon">
            <IconInbox />
          </span>
          <span className="ck-row-main">
            <span className="ck-row-title">Inbox</span>
          </span>
          {waiting > 0 && <span className="ck-count">{waiting}</span>}
        </button>
        {section('Needs you', groups.needs, waiting)}
        {section('Working', groups.working)}
        {section('Idle', groups.idle)}
        {list.length === 0 && c.registry.initialReady && <p className="ck-side-empty">No agents running.</p>}
      </nav>

      <div className="ck-side-foot">
        <SegTabs
          label="Mode"
          value={c.mode}
          onChange={(m) => void c.setMode(m)}
          options={[
            { id: 'orchestration', label: 'Orchestration' },
            { id: 'individual', label: 'Individual' },
          ]}
          small
        />
        {onOpenDoctor && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenDoctor}>
            <IconTerminal />
            Set up agents
          </button>
        )}
        {onOpenMemory && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenMemory}>
            <IconNote />
            Project memory
          </button>
        )}
        <div className="ck-foot-row">
          <button type="button" className="ck-btn ck-btn--plain" onClick={() => void c.openSettings()}>
            <IconSettings />
            Settings
          </button>
          <button
            type="button"
            className="ck-icon-btn"
            onClick={() => void c.toggleTheme()}
            title={c.theme === 'dark' ? 'Switch to light' : 'Switch to dark'}
            aria-label="Toggle theme"
          >
            <span className={`ck-theme-ic${c.theme === 'light' ? ' is-light' : ''}`}>
              {c.theme === 'dark' ? <IconMoon /> : <IconSun />}
            </span>
          </button>
        </div>
        <div className="ck-bridge">{c.bridgeLine}</div>
      </div>
    </aside>
  );
}
