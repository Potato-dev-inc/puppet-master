import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { previewLabel } from '../../lib/pane-preview';
import { pointApi } from '../../lib/point-api';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { requestChatJump } from '../../lib/chatJump';
import { requestBringChats, requestImportedChat } from '../../lib/chatImport';
import type { ChatHit } from '../../lib/chatSearch';
import { useChatSearch } from '../../hooks/useChatSearch';
import { useChatFacets } from '../../hooks/useChatFacets';
import { dropShortcut, hasChatFilters, mergeChatFilters, parseChatQuery, type ChatFilters } from '../../lib/chatFilters';
import '../../styles/cockpit-chat-search.css';
import { ChatHitDetail, ChatHitRows, SEARCH_CHATS_HINT, SEARCH_CHATS_TITLE } from './ChatHits';
import { ChatFilterChips, type ChipKey } from './ChatFilterChips';
import { T as WELCOME } from '../firstrun/copy';
import { DotSpinner } from './DotSpinner';
import { IconChart, IconGlobe, IconLayers, IconMoon, IconNote, IconPlus, IconReflow, IconSearch, IconSettings, IconStop, IconSun, IconTerminal } from './Icons';
import { matchesQuery, spinnerStatus, stateText } from './model';
import { settingsShortcuts } from './palette-shortcuts';
import { useMissionsEnabled } from '../../lib/missions';
import { projectWithCopy } from './shell-labels';
import { multiLoginAgents, paneHeading, paneSubline } from './titles';
import type { CompanionView } from '../../lib/companion';
import { stateWord } from '../../lib/companion';
import { Avatar } from '../companion/Avatar';
import { isCompanionPaneId } from './companion/model';

interface Item {
  id: string;
  group: 'Panes' | 'New agent' | 'Actions';
  title: string;
  desc?: string;
  icon: ReactNode;
  hint?: string;
  /** Extra words that find the row (not shown). */
  keywords?: string;
  run: () => void;
}

export type PaletteMode = 'commands' | 'chats';

export function Palette({
  c,
  initialMode = 'commands',
  onClose,
  onOpenDoctor,
  onStopAll,
  onOpenMemory,
  onOpenPlans,
  onOpenScores,
  onUndoLastTurn,
  onOpenWelcome,
  onStartCrew,
  onOpenShortcuts,
  companions,
  onOpenCompanion,
  onNewCompanion,
}: {
  c: CockpitApi;
  /** `chats`: open straight in "Search all chats" (Ctrl+Shift+F). */
  initialMode?: PaletteMode;
  onClose: () => void;
  onOpenDoctor?: () => void;
  onStopAll?: () => void;
  onOpenMemory?: () => void;
  onOpenPlans?: () => void;
  /** Opens "Scores on your code". Absent: no Scores action. */
  onOpenScores?: () => void;
  /** Opens the restore confirmation for the pane's last turn (never restores by itself). */
  onUndoLastTurn?: (paneId: string) => void;
  /** Shows the first-run welcome again. */
  onOpenWelcome?: () => void;
  /** Opens "Start crew": pick a saved team of agents and a folder. */
  onStartCrew?: () => void;
  /** Opens the keyboard shortcuts sheet. */
  onOpenShortcuts?: () => void;
  /** The companions (null: no companion backend). Each one gets an "Open {name}" row. */
  companions?: CompanionView[] | null;
  onOpenCompanion?: (id: string) => void;
  onNewCompanion?: () => void;
}) {
  const [query, setQuery] = useState('');
  const [index, setIndex] = useState(0);
  const [mode, setMode] = useState<PaletteMode>(initialMode);
  // A hit whose pane is gone, shown read-only.
  const [closedHit, setClosedHit] = useState<ChatHit | null>(null);
  const listing = useAgentRegistry();
  // The chips (picked here) and the typed shortcuts (`agent:`, `in:`, `from:`) make one set of filters.
  const [chips, setChips] = useState<ChatFilters>({});
  const facets = useChatFacets(mode === 'chats');
  const parsed = useMemo(() => parseChatQuery(query, facets), [query, facets]);
  const filters = useMemo(() => mergeChatFilters(chips, parsed.typed), [chips, parsed]);
  const chats = useChatSearch(parsed.text, filters, mode === 'chats');
  // A chip picked for a filter takes over from a typed shortcut for it (the shortcut leaves the box).
  const pickChip = (key: ChipKey, value: string | undefined) => {
    setChips((now) => ({ ...now, [key]: value }) as ChatFilters);
    if (key !== 'when') setQuery((now) => dropShortcut(now, key));
  };
  const listRef = useRef<HTMLDivElement>(null);

  const missionsOn = useMissionsEnabled();
  const items = useMemo<Item[]>(() => {
    const out: Item[] = [];
    const multiLogin = multiLoginAgents(c.registry.paneList.map((p) => p.info));
    for (const companion of companions ?? []) {
      out.push({
        id: `companion:${companion.id}`,
        group: 'Panes',
        title: `Open ${companion.name}`,
        desc: `Companion · ${stateWord(companion.state)}`,
        keywords: 'companion',
        icon: <Avatar id={companion.avatar} size={16} state={companion.state} name={companion.name} />,
        run: () => onOpenCompanion?.(companion.id),
      });
    }
    for (const p of c.registry.paneList) {
      const id = p.info.id;
      // A companion's pane is opened through its "Open {name}" row above, not as a second pane.
      if (isCompanionPaneId(id)) continue;
      const agentName = c.names.get(id) ?? p.info.agent_type;
      const heading = paneHeading(p.info, agentName);
      out.push({
        id: `pane:${id}`,
        group: 'Panes',
        title: heading.primary,
        desc: `${paneSubline(p.info, agentName, projectWithCopy(p.info.cwd), multiLogin)} · ${stateText(p.status)}`,
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
        desc: c.readyAgents.has(e.type) ? 'Opens instantly' : undefined,
        icon: <IconPlus />,
        hint: c.readyAgents.has(e.type) ? 'ready' : undefined,
        run: () => void c.newAgent(e.type),
      });
    }
    if (onNewCompanion) {
      out.push({
        id: 'new:companion',
        group: 'New agent',
        title: 'New companion',
        desc: 'An always-on helper that runs on your agent',
        keywords: 'companion helper',
        icon: <IconPlus />,
        run: onNewCompanion,
      });
    }
    if (c.selectedId && c.registry.panes.has(c.selectedId)) {
      const sel = c.selectedId;
      out.push({
        id: 'act:interrupt',
        group: 'Actions',
        title: 'Stop this agent',
        desc: c.names.get(sel),
        icon: <IconStop />,
        hint: '⌃C',
        run: () => void c.interrupt(sel),
      });
      if (onUndoLastTurn) {
        out.push({
          id: 'act:undo',
          group: 'Actions',
          title: 'Undo last turn…',
          desc: 'Restore files to before the last turn. The agent keeps its memory.',
          icon: <IconReflow />,
          run: () => onUndoLastTurn(sel),
        });
      }
    }
    for (const p of c.registry.paneList) {
      const id = p.info.id;
      for (const pv of c.previews?.[id] ?? []) {
        const agent = c.names.get(id) ?? p.info.agent_type;
        const failed = (e: unknown) => c.notify(typeof e === 'string' ? e : 'Could not open the preview');
        out.push({
          id: `preview:${id}:${pv.port}`,
          group: 'Actions',
          title: `Open preview: ${agent} ${previewLabel(pv)}`,
          desc: pv.url,
          icon: <IconGlobe />,
          run: () => void pointApi.openPreview(id, pv.port).catch(failed),
        });
        out.push({
          id: `point:${id}:${pv.port}`,
          group: 'Actions',
          title: `Point and fix in the preview: ${agent} ${previewLabel(pv)}`,
          desc: 'Click the part that is wrong, then say what should change',
          keywords: 'point fix pick element change',
          icon: <IconGlobe />,
          run: () => void pointApi.openPreview(id, pv.port, true).catch(failed),
        });
      }
    }
    out.push({
      id: 'act:search-chats',
      group: 'Actions',
      title: SEARCH_CHATS_TITLE,
      desc: 'Find an answer from any agent, any day',
      keywords: 'find history conversation messages',
      icon: <IconSearch />,
      hint: SEARCH_CHATS_HINT,
      run: () => {
        setMode('chats');
        setQuery('');
      },
    });
    out.push({
      id: 'act:bring-chats',
      group: 'Actions',
      title: 'Bring my old chats',
      desc: 'Read the chats you had in other agents, into search. Nothing leaves this PC.',
      keywords: 'import old chats claude codex opencode history bring read',
      icon: <IconNote />,
      run: requestBringChats,
    });
    if (onStopAll) {
      out.push({
        id: 'act:stop-all',
        group: 'Actions',
        title: 'Stop all agents',
        desc: 'Stops every agent that is working. Their conversations stay open.',
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
    if (onOpenWelcome) {
      out.push({
        id: 'act:welcome',
        group: 'Actions',
        title: WELCOME.paletteTitle,
        desc: WELCOME.paletteDesc,
        icon: <IconNote />,
        run: onOpenWelcome,
      });
    }
    if (onStartCrew) {
      out.push({
        id: 'act:start-crew',
        group: 'Actions',
        title: 'Start crew…',
        desc: 'Start a saved team of agents in a folder',
        keywords: 'crew team helpers saved template',
        icon: <IconLayers />,
        run: onStartCrew,
      });
    }
    if (onOpenShortcuts) {
      out.push({
        id: 'act:shortcuts',
        group: 'Actions',
        title: 'Keyboard shortcuts',
        desc: 'Every key that works in the app',
        keywords: 'keys keyboard hotkeys help shortcut',
        icon: <IconNote />,
        hint: '?',
        run: onOpenShortcuts,
      });
    }
    if (onOpenMemory) {
      out.push({
        id: 'act:memory',
        group: 'Actions',
        title: 'Project…',
        desc: 'Memory every agent reads, and how copies get ready',
        keywords: 'project memory copies setup',
        icon: <IconNote />,
        run: onOpenMemory,
      });
    }
    if (onOpenPlans) {
      out.push({
        id: 'act:plans',
        group: 'Actions',
        title: 'Your plans',
        desc: 'What each plan did this month: turns, time, files and limits',
        keywords: 'plans month usage limit subscription account turns time',
        icon: <IconChart />,
        run: onOpenPlans,
      });
    }
    if (onOpenScores) {
      out.push({
        id: 'act:scores',
        group: 'Actions',
        title: 'Scores on your code',
        desc: 'Which agent works best here, from your own history',
        keywords: 'scores best agent compare results history',
        icon: <IconChart />,
        run: onOpenScores,
      });
    }
    out.push(...settingsShortcuts(c, { missionsOn, projectPath: c.selectedId ? (c.registry.panes.get(c.selectedId)?.info?.cwd ?? null) : null }));
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
        run: () => void c.openSettings('bridge'),
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
  }, [c, onOpenDoctor, onStopAll, onOpenMemory, onOpenPlans, onOpenScores, onUndoLastTurn, onOpenWelcome, onStartCrew, onOpenShortcuts, missionsOn, companions, onOpenCompanion, onNewCompanion]);

  const filtered = useMemo(
    () => items.filter((i) => matchesQuery(`${i.title} ${i.desc ?? ''} ${i.keywords ?? ''} ${i.group}`, query)),
    [items, query],
  );

  const count = mode === 'chats' ? chats.hits.length : filtered.length;
  useEffect(() => setIndex(0), [query, mode, filters]);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [index, filtered, chats.hits]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    // "Search all chats" switches the palette over and keeps it open.
    if (item.id === 'act:search-chats') {
      item.run();
      return;
    }
    onClose();
    item.run();
  };

  // Opens the pane's Chat at the message; a pane that is gone shows what was kept.
  const chooseHit = (hit: ChatHit | undefined) => {
    if (!hit) return;
    // An old chat the person brought in has no pane: it opens read-only in its own sheet, at the message.
    if (hit.imported) {
      onClose();
      requestImportedChat(hit.agent, hit.session, hit.itemId);
      return;
    }
    if (!c.registry.panes.has(hit.paneId)) {
      setClosedHit(hit);
      return;
    }
    onClose();
    requestChatJump(hit.paneId, hit.itemId);
    c.select(hit.paneId);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setIndex((i) => (count ? (i + 1) % count : 0));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setIndex((i) => (count ? (i - 1 + count) % count : 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (mode === 'chats') chooseHit(chats.hits[index]);
      else choose(filtered[index]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      if (closedHit) setClosedHit(null);
      else onClose();
    } else if (e.key === 'Backspace' && mode === 'chats' && !query) {
      // Nothing left to delete: back to the commands.
      e.preventDefault();
      setMode('commands');
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
          placeholder={mode === 'chats' ? 'Search what was said · agent:, in:, from:' : 'Switch pane, start an agent, run an action'}
          aria-label={mode === 'chats' ? SEARCH_CHATS_TITLE : 'Search commands'}
          role="combobox"
          aria-expanded="true"
          aria-controls="ck-palette-list"
        />
        {mode === 'chats' && (
          <div className="ck-palette-mode">
            <span>{SEARCH_CHATS_TITLE}</span>
            <span className="ck-palette-mode-note">Chats you have opened</span>
          </div>
        )}
        {mode === 'chats' && !closedHit && (
          <ChatFilterChips filters={filters} facets={facets} agentName={(id) => agentLabel(listing, id)} onPick={pickChip} />
        )}
        {closedHit && (
          <ChatHitDetail hit={closedHit} agentName={agentLabel(listing, closedHit.agent)} now={Date.now()} onBack={() => setClosedHit(null)} />
        )}
        <div className="ck-palette-list" id="ck-palette-list" role="listbox" ref={listRef} hidden={!!closedHit}>
          {mode === 'chats' && (
            <>
              {chats.hits.length === 0 && (
                <p className="ck-sub ck-pad">
                  {!query.trim() && !hasChatFilters(filters)
                    ? 'Type to search every chat you have opened.'
                    : chats.state === 'searching'
                      ? 'Searching…'
                      : chats.state === 'failed'
                        ? 'Search is not available right now.'
                        : chats.state === 'off'
                          ? 'Search is off — turn it on in Settings'
                          : 'No matches'}
                </p>
              )}
              <ChatHitRows
                hits={chats.hits}
                index={index}
                agentName={(id) => agentLabel(listing, id)}
                onHover={setIndex}
                onChoose={chooseHit}
                now={Date.now()}
              />
            </>
          )}
          {mode === 'commands' && filtered.length === 0 && <p className="ck-sub ck-pad">No matches</p>}
          {mode === 'commands' && filtered.map((item, i) => {
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
