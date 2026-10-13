import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { cockpitApi } from '../../lib/cockpit-api';
import { projectName } from '../../lib/mobile-agent-tabs';
import { openPreview, previewLabel } from '../../lib/pane-preview';
import { DotSpinner } from './DotSpinner';
import { IconGlobe, IconLayers, IconMoon, IconPlus, IconReflow, IconSettings, IconStop, IconSun, IconTerminal } from './Icons';
import { matchesQuery, spinnerStatus, stateText } from './model';
import { paneHeading } from './titles';

interface Item {
  id: string;
  group: 'Panes' | 'New agent' | 'Actions';
  title: string;
  desc?: string;
  icon: ReactNode;
  hint?: string;
  run: () => void;
}

export function Palette({
  c,
  onClose,
  onOpenDoctor,
  onStopAll,
}: {
  c: CockpitApi;
  onClose: () => void;
  onOpenDoctor?: () => void;
  onStopAll?: () => void;
}) {
  const [query, setQuery] = useState('');
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const items = useMemo<Item[]>(() => {
    const out: Item[] = [];
    for (const p of c.registry.paneList) {
      const id = p.info.id;
      const heading = paneHeading(p.info, c.names.get(id) ?? p.info.agent_type);
      out.push({
        id: `pane:${id}`,
        group: 'Panes',
        title: heading.primary,
        desc: `${heading.secondary ? `${heading.secondary} · ` : ''}${projectName(p.info.cwd)} · ${stateText(p.status)}`,
        icon: <DotSpinner status={spinnerStatus(p.status)} />,
        run: () => c.select(id),
      });
    }
    for (const e of c.launchEntries) {
      if (e.disabled) continue;
      out.push({
        id: `new:${e.type}`,
        group: 'New agent',
        title: `New ${e.label}`,
        desc: c.readyAgents.has(e.type) ? 'Warm standby ready' : undefined,
        icon: <IconPlus />,
        hint: c.readyAgents.has(e.type) ? 'ready' : undefined,
        run: () => void c.newAgent(e.type),
      });
    }
    if (c.selectedId && c.registry.panes.has(c.selectedId)) {
      const sel = c.selectedId;
      out.push({
        id: 'act:interrupt',
        group: 'Actions',
        title: 'Interrupt current pane',
        desc: c.names.get(sel),
        icon: <IconStop />,
        hint: '⌃C',
        run: () => void c.interrupt(sel),
      });
      out.push({
        id: 'act:undo',
        group: 'Actions',
        title: 'Undo last turn',
        desc: 'Restore files to before the last turn. The agent keeps its memory.',
        icon: <IconReflow />,
        run: () =>
          void cockpitApi
            .checkpointUndoLast(sel)
            .then(() => c.notify('Files restored to before the last turn. Your previous files are saved in History.'))
            .catch((err) => c.notify(err instanceof Error ? err.message : String(err))),
      });
    }
    for (const p of c.registry.paneList) {
      const id = p.info.id;
      for (const pv of c.previews?.[id] ?? []) {
        const agent = c.names.get(id) ?? p.info.agent_type;
        out.push({
          id: `preview:${id}:${pv.port}`,
          group: 'Actions',
          title: `Open preview: ${agent} ${previewLabel(pv)}`,
          desc: pv.url,
          icon: <IconGlobe />,
          run: () => void openPreview(pv.url).catch(() => c.notify('Could not open the preview')),
        });
      }
    }
    if (onStopAll) {
      out.push({
        id: 'act:stop-all',
        group: 'Actions',
        title: 'Stop all agents',
        desc: 'Interrupt every agent that is working',
        icon: <IconStop />,
        run: onStopAll,
      });
    }
    if (onOpenDoctor) {
      out.push({
        id: 'act:doctor',
        group: 'Actions',
        title: 'Set up agents',
        desc: 'Check which agents work and fix the ones that do not',
        icon: <IconTerminal />,
        run: onOpenDoctor,
      });
    }
    out.push(
      {
        id: 'act:settings',
        group: 'Actions',
        title: 'Open settings',
        icon: <IconSettings />,
        run: () => void c.openSettings(),
      },
      {
        id: 'act:toolbelt',
        group: 'Actions',
        title: 'Toolbelt…',
        desc: 'MCP servers for every agent',
        icon: <IconLayers />,
        run: () => void cockpitApi.openSettings('mcp').catch(() => {}),
      },
      {
        id: 'act:mode',
        group: 'Actions',
        title: c.mode === 'orchestration' ? 'Switch to Individual mode' : 'Switch to Orchestration mode',
        desc: c.mode === 'orchestration' ? 'Every pane in its own window' : 'Every pane in this window',
        icon: <IconLayers />,
        run: () => void c.setMode(c.mode === 'orchestration' ? 'individual' : 'orchestration'),
      },
      {
        id: 'act:theme',
        group: 'Actions',
        title: c.theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme',
        icon: c.theme === 'dark' ? <IconSun /> : <IconMoon />,
        run: () => void c.toggleTheme(),
      },
    );
    return out;
  }, [c, onOpenDoctor, onStopAll]);

  const filtered = useMemo(
    () => items.filter((i) => matchesQuery(`${i.title} ${i.desc ?? ''} ${i.group}`, query)),
    [items, query],
  );

  useEffect(() => setIndex(0), [query]);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [index, filtered]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    onClose();
    item.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setIndex((i) => (filtered.length ? (i + 1) % filtered.length : 0));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setIndex((i) => (filtered.length ? (i - 1 + filtered.length) % filtered.length : 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      choose(filtered[index]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    }
  };

  let lastGroup = '';
  return (
    <div className="ck-scrim ck-scrim--top" onMouseDown={onClose}>
      <div className="ck-palette" role="dialog" aria-label="Command palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          className="ck-palette-input"
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Switch pane, start an agent, run an action"
          aria-label="Search commands"
          role="combobox"
          aria-expanded="true"
          aria-controls="ck-palette-list"
        />
        <div className="ck-palette-list" id="ck-palette-list" role="listbox" ref={listRef}>
          {filtered.length === 0 && <p className="ck-sub ck-pad">No matches</p>}
          {filtered.map((item, i) => {
            const header = item.group !== lastGroup ? item.group : null;
            lastGroup = item.group;
            return (
              <div key={item.id}>
                {header && <div className="ck-caption">{header}</div>}
                <div
                  role="option"
                  aria-selected={i === index}
                  data-active={i === index}
                  className={`ck-palette-row${i === index ? ' is-active' : ''}`}
                  onMouseMove={() => setIndex(i)}
                  onClick={() => choose(item)}
                >
                  <span className="ck-palette-ic">{item.icon}</span>
                  <span className="ck-palette-title">{item.title}</span>
                  {item.desc && <span className="ck-palette-desc">{item.desc}</span>}
                  {item.hint && <kbd className="ck-kbd">{item.hint}</kbd>}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
