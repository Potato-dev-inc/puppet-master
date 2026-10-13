import { useEffect, useRef, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { projectName } from '../../lib/mobile-agent-tabs';
import { activityText } from '../../lib/pane-activity';
import { mainPreview, previewLabel } from '../../lib/pane-preview';
import '../../styles/live-preview.css';
import { DotSpinner } from './DotSpinner';
import { IconGlobe, IconInbox, IconMoon, IconPlus, IconSearch, IconSettings, IconSun, IconTerminal } from './Icons';
import { formatElapsed, groupPanes, spinnerStatus } from './model';
import { SegTabs } from './SegTabs';

export function Sidebar({
  c,
  onOpenPalette,
  onOpenDoctor,
}: {
  c: CockpitApi;
  onOpenPalette: () => void;
  onOpenDoctor?: () => void;
}) {
  const list = c.registry.paneList;
  const groups = groupPanes(list);
  const waiting = list.filter((p) => p.status === 'waiting_input').length;
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

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

  const section = (title: string, items: typeof list) =>
    items.length === 0 ? null : (
      <div className="ck-group" key={title}>
        <div className="ck-caption">{title}</div>
        {items.map((p) => {
          const id = p.info.id;
          const since = c.statusSince.get(id);
          const active = c.selectedId === id && !c.spawn;
          return (
            <button
              key={id}
              type="button"
              className={`ck-row${active ? ' is-active' : ''}${p.status === 'waiting_input' ? ' is-needs' : ''}`}
              onClick={() => c.select(id)}
              aria-current={active ? 'true' : undefined}
            >
              <span className="ck-row-glyph">
                <DotSpinner status={spinnerStatus(p.status)} />
              </span>
              <span className="ck-row-main" title={projectName(p.info.cwd)}>
                <span className="ck-row-title">{c.names.get(id) ?? p.info.agent_type}</span>
                <span className="ck-row-sub">
                  {activityText(p.status, c.activities?.[id]) ?? projectName(p.info.cwd)}
                </span>
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
            <div className="ck-menu ck-menu--left" role="menu">
              <div className="ck-menu-cap">New agent</div>
              {c.launchEntries.map((e) => (
                <button
                  key={e.type}
                  type="button"
                  role="menuitem"
                  className="ck-menu-item"
                  disabled={e.disabled}
                  title={e.disabledReason}
                  onClick={() => {
                    setMenuOpen(false);
                    void c.newAgent(e.type);
                  }}
                >
                  <span>{e.label}</span>
                  {!e.disabled && c.readyAgents.has(e.type) && <span className="ck-menu-hint">ready</span>}
                  {e.disabled && <span className="ck-menu-hint">headless</span>}
                </button>
              ))}
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
        {section('Needs you', groups.needs)}
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
