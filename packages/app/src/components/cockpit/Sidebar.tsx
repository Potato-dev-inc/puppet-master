import { useContext, useEffect, useRef, useState } from 'react';
import { NewAgentMenu, type ProjectChoice } from './NewAgentMenu';
import { shipApi, type GitState, type ShipApi } from '../../lib/ship-api';
import { tauri } from '../../lib/tauri';
import { isValidProjectPath } from '../../lib/project-path';
import { copyDefault, lastRealFolder, projectOfCwd, readStoredCopyPreference, supportsCopies, writeCopyPreference } from '../../lib/ship-model';
import type { CockpitApi } from '../../hooks/useCockpit';
import { useNeedsYou } from '../../hooks/useNeedsYou';
import { blockingItems, itemsOfKind, needsYouPaneIds } from '../../lib/attention';
import { activityText } from '../../lib/pane-activity';
import { mainPreview, type PreviewUrl } from '../../lib/pane-preview';
import '../../styles/live-preview.css';
import '../../styles/cockpit-helpers.css';
import { DotSpinner } from './DotSpinner';
import { IconChart, IconGlobe, IconInbox, IconMoon, IconNote, IconPlus, IconSearch, IconSettings, IconSun, IconTerminal } from './Icons';
import { formatElapsed, spinnerStatus } from './model';
import { groupHelperTree, helperChipText, type HelperNode } from '../../lib/pane-helpers';
import { multiLoginAgents, paneHeading, paneSubline } from './titles';
import { LastTurnContext } from './away-context';
import { SegTabs } from './SegTabs';
import { projectWithCopy } from './shell-labels';
import type { CompanionView } from '../../lib/companion';
import { CompanionRow, MakeCompanionRow } from './companion/CompanionRow';
import { isCompanionPaneId } from './companion/model';
import { JobsChip } from './rail/JobsChip';

/** `localhost:5173`: where the preview is, without the scheme. */
const previewAddress = (p: PreviewUrl): string => p.url.replace(/^https?:\/\//, '').replace(/\/$/, '');
import { StartMissionSheet } from './mission/StartMissionSheet';
import { onOpenStartMission, useMissions, useMissionsEnabled, isActive } from '../../lib/missions';
import { NightChip } from './night/NightChip';
import { TonightHost } from './night/TonightHost';
import { onOpenTonight } from '../../lib/night-host';
import { useNightEnabled } from '../../lib/night';

export function Sidebar({
  c,
  onOpenPalette,
  onOpenDoctor,
  onOpenMemory,
  onOpenPlans,
  onOpenScores,
  onOpenMorning,
  onOpenStore,
  companions,
  openCompanionId = null,
  onOpenCompanion,
  onNewCompanion,
  onOpenToSide,
  ship = shipApi,
}: {
  c: CockpitApi;
  onOpenPalette: () => void;
  onOpenDoctor?: () => void;
  onOpenMemory?: () => void;
  onOpenPlans?: () => void;
  /** Opens "Scores on your code" (which agent works best in the selected pane's project). */
  onOpenScores?: () => void;
  /** Night Shift: opens the Morning page. Absent: no Night Shift chip. */
  onOpenMorning?: () => void;
  /** The "More agents…" row of the New agent menu opens the Agent Store. Absent: no row. */
  onOpenStore?: () => void;
  /** A pane's row menu: "Open to the side" (split view). Absent: the row has no menu. */
  onOpenToSide?: (paneId: string) => void;
  /** The companions, above Inbox. Absent or null: no companion rows (no backend, or not read yet). */
  companions?: CompanionView[] | null;
  /** The companion whose page is open, if any: its row is the active one. */
  openCompanionId?: string | null;
  onOpenCompanion?: (id: string) => void;
  onNewCompanion?: () => void;
  ship?: ShipApi;
}) {
  const list = c.registry.paneList;
  // A companion's pane has its own row above the Inbox, so it is not listed in these groups (SPEC section 8).
  const panes = list.filter((p) => !isCompanionPaneId(p.info.id));
  // The same list the Inbox badge and the Doto counter use, so the three numbers always agree.
  const needs = useNeedsYou(c);
  const questionPanes = needsYouPaneIds(needs);
  const groups = groupHelperTree(panes, questionPanes);
  // Parents whose helpers the person folded away (the helpers that need you stay listed in Needs you).
  const [folded, setFolded] = useState<ReadonlySet<string>>(new Set());
  const toggleFolded = (id: string) =>
    setFolded((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const waiting = blockingItems(needs).length;
  // Questions agents asked through ask_user: they belong to no pane row, so they get a row of their own.
  const asks = itemsOfKind(blockingItems(needs), 'question').length;
  const multiLogin = multiLoginAgents(list.map((p) => p.info));
  const lastTurn = useContext(LastTurnContext);
  const [menuOpen, setMenuOpen] = useState(false);
  // The right-click menu of a pane row: where it opened, and which pane it is for.
  const [rowMenu, setRowMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const rowMenuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!rowMenu) return;
    const away = (e: MouseEvent) => {
      if (!rowMenuRef.current?.contains(e.target as Node)) setRowMenu(null);
    };
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setRowMenu(null);
    window.addEventListener('mousedown', away);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('keydown', esc);
    };
  }, [rowMenu]);
  // The folder step: an explicit choice wins, else the selected pane's project.
  const [chosen, setChosen] = useState<ProjectChoice | null>(null);
  const selectedCwd = c.selectedId ? c.registry.panes.get(c.selectedId)?.info.cwd : undefined;
  // With neither, new agents start in the open project (the app's default folder): name it too.
  const [openProject, setOpenProject] = useState<string | null>(null);
  useEffect(() => {
    if (!menuOpen) return;
    let live = true;
    tauri
      .getProjectPath()
      .then((path) => live && setOpenProject(isValidProjectPath(path) ? path : null))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [menuOpen]);
  // The newest real folder the person used: "+" starts there when no pane names a project.
  const [lastFolder, setLastFolder] = useState<ProjectChoice | null>(null);
  useEffect(() => {
    let live = true;
    ship
      .recent()
      .then((list) => live && setLastFolder(lastRealFolder(list)))
      .catch(() => undefined); // no desktop shell: the open project is used
    return () => {
      live = false;
    };
  }, [menuOpen, ship]);
  const base: ProjectChoice | null = chosen ?? projectOfCwd(selectedCwd) ?? lastFolder ?? projectOfCwd(openProject);
  // A folder taken from a pane has no Git state yet: ask once, so the Separate copy switch knows.
  const [looked, setLooked] = useState<{ path: string; state: GitState } | null>(null);
  useEffect(() => {
    if (!base || base.state) return;
    let live = true;
    ship
      .gitState(base.path)
      .then((state) => live && setLooked({ path: base.path, state }))
      .catch(() => undefined); // no desktop shell (a browser): the switch stays as it is
    return () => {
      live = false;
    };
  }, [base?.path, base?.state, ship]); // eslint-disable-line react-hooks/exhaustive-deps
  const project: ProjectChoice | null = base ? { ...base, state: base.state ?? (looked?.path === base.path ? looked.state : undefined) } : null;
  const projectKey = project?.path ?? null;
  // The person's remembered choice for this project wins; with none, off: the agent works in the folder itself.
  const [stored, setStored] = useState(() => readStoredCopyPreference(projectKey));
  useEffect(() => setStored(readStoredCopyPreference(projectKey)), [projectKey]);
  const isolated = copyDefault(stored);
  const menuRef = useRef<HTMLDivElement>(null);
  // Missions (Preview, Labs): "+" → Start a mission… opens the start sheet for the menu's project.
  const missionsOn = useMissionsEnabled();
  const missions = useMissions();
  const [missionFor, setMissionFor] = useState<ProjectChoice | null>(null);
  useEffect(() => onOpenStartMission((path) => {
    const p = path ? projectOfCwd(path) : project;
    if (p) setMissionFor(p);
  }), [project]);
  // Night Shift (Labs): "+" → Tonight… and the palette open the Tonight sheet; the chip below shows a run.
  const nightOn = useNightEnabled();
  const [tonightOpen, setTonightOpen] = useState(false);
  useEffect(() => onOpenTonight(() => setTonightOpen(true)), []);
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

  type Pane = (typeof list)[number];
  const paneTitle = (p: Pane) => paneHeading(p.info, c.names.get(p.info.id) ?? p.info.agent_type).primary;
  const row = (p: Pane, awayFrom?: Pane) => {
    const id = p.info.id;
    const since = c.statusSince.get(id);
    const active = c.selectedId === id && !c.spawn && !openCompanionId;
    const heading = paneHeading(p.info, c.names.get(id) ?? p.info.agent_type);
    const detail =
      activityText(p.status, c.activities?.[id]) ??
      (p.status === 'error' ? `Error · ${projectWithCopy(p.info.cwd)}` : projectWithCopy(p.info.cwd));
    const subline = paneSubline(
      p.info,
      c.names.get(id) ?? p.info.agent_type,
      // Away from its parent (in Needs you) a helper says whose it is.
      awayFrom ? `Helper of ${paneTitle(awayFrom)} · ${detail}` : detail,
      multiLogin,
    );
    return (
      <button
        key={id}
        type="button"
        className={`ck-row${active ? ' is-active' : ''}${p.status === 'waiting_input' || questionPanes.has(id) ? ' is-needs' : ''}`}
        onClick={() => c.select(id)}
        onContextMenu={
          onOpenToSide
            ? (e) => {
                e.preventDefault();
                setRowMenu({ id, x: e.clientX, y: e.clientY });
              }
            : undefined
        }
        title={lastTurn.get(id)?.sentence}
        aria-current={active ? 'true' : undefined}
      >
        <span className="ck-row-glyph">
          <DotSpinner status={spinnerStatus(p.status)} />
        </span>
        <span className="ck-row-main" title={[...new Set([heading.primary, subline, projectWithCopy(p.info.cwd)])].join(' · ')}>
          <span className="ck-row-title">{heading.primary}</span>
          <span className="ck-row-sub">{subline}</span>
        </span>
        {mainPreview(c.previews, id) && (
          <span className="ck-row-preview" title={`Has a preview · ${previewAddress(mainPreview(c.previews, id)!)}`} role="img" aria-label="Has a preview">
            <IconGlobe />
          </span>
        )}
        <span className="ck-row-time">{since ? formatElapsed(c.now - since) : ''}</span>
      </button>
    );
  };
  // A row, and under it its helpers: indented behind a thin guide line, folded by the "N helpers" chip.
  const node = (n: HelperNode<Pane>): JSX.Element => {
    const id = n.pane.info.id;
    if (n.helpers.length === 0) return row(n.pane, n.awayFrom);
    const open = !folded.has(id);
    return (
      <div key={id} className="ck-node">
        {row(n.pane, n.awayFrom)}
        <div className="ck-helpers" role="group" aria-label={`Helpers of ${paneTitle(n.pane)}`}>
          <button
            type="button"
            className="ck-helper-chip"
            onClick={() => toggleFolded(id)}
            aria-expanded={open}
            title={open ? 'Hide helpers' : 'Show helpers'}
          >
            {helperChipText(n)}
            <span className={`ck-helper-caret${open ? ' is-open' : ''}`} aria-hidden="true" />
          </button>
          {open && n.helpers.map(node)}
        </div>
      </div>
    );
  };

  const section = (title: string, items: HelperNode<Pane>[], count?: number, extraRow = 0) =>
    items.length === 0 && extraRow === 0 ? null : (
      <div className="ck-group" key={title}>
        <div className="ck-caption">
          {title}
          {count ? <span className="ck-caption-count">{count}</span> : null}
        </div>
        {items.map(node)}
        {extraRow > 0 && (
          <button type="button" className="ck-row" onClick={() => c.select(null)}>
            <span className="ck-row-glyph">
              <DotSpinner status="waiting_input" />
            </span>
            <span className="ck-row-main">
              <span className="ck-row-title">Questions from agents</span>
            </span>
            <span className="ck-row-time">{extraRow}</span>
          </button>
        )}
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
                  setStored(on);
                  writeCopyPreference(projectKey, on);
                }}
                ship={ship}
                onOpenStore={
                  onOpenStore
                    ? () => {
                        setMenuOpen(false);
                        onOpenStore();
                      }
                    : undefined
                }
                onStartMission={
                  missionsOn && project
                    ? () => {
                        setMenuOpen(false);
                        setMissionFor(project);
                      }
                    : undefined
                }
                onTonight={
                  nightOn
                    ? () => {
                        setMenuOpen(false);
                        setTonightOpen(true);
                      }
                    : undefined
                }
              />
            </div>
          )}
        </div>
      </div>

      <nav className="ck-side-scroll">
        {companions &&
          (companions.length === 0
            ? onNewCompanion && <MakeCompanionRow onMake={onNewCompanion} />
            : companions.map((companion) => (
                <CompanionRow
                  key={companion.id}
                  companion={companion}
                  active={openCompanionId === companion.id}
                  onOpen={() => onOpenCompanion?.(companion.id)}
                />
              )))}
        <button
          type="button"
          className={`ck-row ck-row--flat${c.selectedId === null && !c.spawn && !openCompanionId ? ' is-active' : ''}`}
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
        {section('Needs you', groups.needs, waiting, asks)}
        {section('Working', groups.working)}
        {section('Idle', groups.idle)}
        {list.length === 0 && c.registry.initialReady && <p className="ck-side-empty">No agents running.</p>}
      </nav>

      {rowMenu && onOpenToSide && (
        <div ref={rowMenuRef} className="ck-menu ck-menu--fixed" role="menu" style={{ left: rowMenu.x, top: rowMenu.y }}>
          <button
            type="button"
            role="menuitem"
            className="ck-menu-item"
            onClick={() => {
              const id = rowMenu.id;
              setRowMenu(null);
              onOpenToSide(id);
            }}
          >
            <span>Open to the side</span>
          </button>
        </div>
      )}

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
        <JobsChip names={c.names} onOpenPane={(id) => c.select(id)} />
        {onOpenMorning && <NightChip onOpenMorning={onOpenMorning} />}
        {onOpenDoctor && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenDoctor}>
            <IconTerminal />
            Set up agents
          </button>
        )}
        {onOpenMemory && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenMemory}>
            <IconNote />
            Project
          </button>
        )}
        {onOpenPlans && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenPlans}>
            <IconChart />
            Your plans
          </button>
        )}
        {onOpenScores && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenScores}>
            <IconChart />
            Scores
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
        {c.bridgeLine && (
          <div className="ck-bridge" role="status">
            <span>{c.bridgeLine}</span>
            <span aria-hidden="true"> · </span>
            <button type="button" className="ck-bridge-fix" onClick={() => void c.openSettings('bridge')}>
              Fix
            </button>
          </div>
        )}
      </div>
      {tonightOpen && nightOn && (
        <TonightHost
          c={c}
          onClose={() => setTonightOpen(false)}
          onStarted={() => c.notify('Night Shift is set. The chip below shows how it goes.')}
        />
      )}
      {missionFor && (
        <StartMissionSheet
          project={{ path: missionFor.path, name: missionFor.name, git: supportsCopies(missionFor.state) }}
          entries={c.launchEntries}
          activeMissions={missions.missions.filter(isActive).length}
          onClose={() => setMissionFor(null)}
          onStarted={() => {
            setMissionFor(null);
            missions.reload();
            c.notify('Mission started. It shows in the Inbox while it works.');
          }}
        />
      )}
    </aside>
  );
}
