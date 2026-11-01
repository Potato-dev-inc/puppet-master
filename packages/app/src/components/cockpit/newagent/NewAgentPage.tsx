// The New agent page: a greeting, one big message box, and under it the choices that matter (folder, agent, how it
// runs). Enter starts the agent in that folder with the message through the same start path as the "+" menu.
// It takes plain data and callbacks; `NewAgentHost` (next to it) wires them to the cockpit. Choices the app has no
// backing for (a permission mode, a model, an effort) are not drawn: nothing here is a stand-in.
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import type { Account } from '../../../lib/accounts';
import type { AgentDescriptor } from '../../../lib/agentRegistry';
import type { RecentProject } from '../../../lib/ship-api';
import { lastRealFolder, projectOfCwd, readStoredCopyPreference, supportsCopies, writeCopyPreference, copyDefault } from '../../../lib/ship-model';
import { IconArrowUp, IconChevronDown, IconFolder, IconPlus, IconSpark } from '../Icons';
import type { ProjectChoice } from '../NewAgentMenu';
import { tauriNewAgentApi, type HomeSettings, type NewAgentApi } from './api';
import { HomeCards, type CardData } from './HomeCards';
import {
  EXAMPLE_MS,
  agentChoices,
  chatAvailable,
  cleanName,
  defaultChoice,
  defaultRunAs,
  exampleAt,
  greeting,
  nextExample,
  sendBlock,
  withPaths,
  type AgentChoice,
  type HomeCard,
  type LaneView,
  type RunAs,
  type StartRequest,
} from './model';
import '../../../styles/cockpit-newagent.css';

const REMEMBER_KEY = 'pm.newagent.agent';

function remembered(): string | null {
  try {
    return localStorage.getItem(REMEMBER_KEY);
  } catch {
    return null;
  }
}
function remember(key: string): void {
  try {
    localStorage.setItem(REMEMBER_KEY, key);
  } catch {
    /* storage can be blocked: the choice just is not remembered */
  }
}

export interface PaneBrief {
  id: string;
  title: string;
  status: string;
}

export interface NewAgentPageProps {
  agents: readonly AgentDescriptor[];
  /** Every agent pane, for the Running and Needs you cards. */
  panes: readonly PaneBrief[];
  /** Panes with something waiting on the person. */
  needs: readonly PaneBrief[];
  /** The folder of the pane that is selected, if any: the first guess for the folder. */
  selectedCwd?: string | null;
  api?: NewAgentApi;
  /** Hand the request to whoever starts agents. May resolve later; the page is busy until then. */
  onStart: (request: StartRequest) => Promise<unknown> | void;
  onOpenPane: (paneId: string) => void;
  /** "Set up an agent": opens the agent doctor. Absent: no such row. */
  onSetUp?: () => void;
}

type Menu = 'plus' | 'folder' | 'agent' | null;

export function NewAgentPage({ agents, panes, needs, selectedCwd = null, api = tauriNewAgentApi, onStart, onOpenPane, onSetUp }: NewAgentPageProps) {
  const [prompt, setPrompt] = useState('');
  const [exampleIdx, setExampleIdx] = useState(0);
  const [menu, setMenu] = useState<Menu>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [home, setHome] = useState<HomeSettings>({ name: '', cards: [] });
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [lanes, setLanes] = useState<LaneView | null>(null);
  const [recent, setRecent] = useState<RecentProject[] | null>(null);
  const [defaultFolder, setDefaultFolder] = useState<string | null>(null);
  const [chosenFolder, setChosenFolder] = useState<ProjectChoice | null>(null);
  const [looked, setLooked] = useState<{ path: string; state: RecentProject['state'] } | null>(null);
  const [pickKey, setPickKey] = useState<string | null>(null);
  const [runPick, setRunPick] = useState<RunAs | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // What the page reads once. Every read may fail (a browser preview, an older build): the page still works.
  useEffect(() => {
    let live = true;
    const ok = <T,>(p: Promise<T>, set: (v: T) => void) => p.then((v) => live && set(v)).catch(() => undefined);
    void ok(api.loadHome(), setHome);
    void ok(api.accounts(), setAccounts);
    void ok(api.lanes(), setLanes);
    void ok(api.recent(), setRecent);
    void ok(api.defaultFolder(), setDefaultFolder);
    let off: (() => void) | undefined;
    api
      .onHomeChanged((next) => live && setHome((h) => ({ ...h, ...next })))
      .then((stop) => (live ? (off = stop) : stop()))
      .catch(() => undefined);
    return () => {
      live = false;
      off?.();
    };
  }, [api]);

  // The example changes while the box is empty.
  useEffect(() => {
    if (prompt) return undefined;
    const timer = window.setInterval(() => setExampleIdx((i) => nextExample(i)), EXAMPLE_MS);
    return () => window.clearInterval(timer);
  }, [prompt]);

  // Outside click and Escape close the open menu.
  const menuRoot = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return undefined;
    const away = (e: MouseEvent) => {
      if (!menuRoot.current?.contains(e.target as Node)) setMenu(null);
    };
    const esc = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') {
        setMenu(null);
        area.current?.focus();
      }
    };
    window.addEventListener('mousedown', away);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('keydown', esc);
    };
  }, [menu]);

  // ----- the folder: the person's pick, else the selected pane's project, else the newest recent one, else the default -----
  const base: ProjectChoice | null = chosenFolder ?? projectOfCwd(selectedCwd) ?? lastRealFolder(recent ?? []) ?? projectOfCwd(defaultFolder);
  useEffect(() => {
    if (!base || base.state) return undefined;
    let live = true;
    api
      .gitState(base.path)
      .then((state) => live && setLooked({ path: base.path, state }))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [api, base?.path, base?.state]); // eslint-disable-line react-hooks/exhaustive-deps
  const folder: ProjectChoice | null = base ? { ...base, state: base.state ?? (looked?.path === base.path ? looked.state : undefined) } : null;
  const folderKey = folder?.path ?? null;
  const [storedCopy, setStoredCopy] = useState(() => readStoredCopyPreference(folderKey));
  useEffect(() => setStoredCopy(readStoredCopyPreference(folderKey)), [folderKey]);
  const copiesOk = supportsCopies(folder?.state);

  // ----- the agent -----
  const choices = useMemo(() => agentChoices(agents, accounts), [agents, accounts]);
  const choice: AgentChoice | null = (pickKey ? choices.find((c) => c.key === pickKey) : undefined) ?? defaultChoice(choices, remembered());
  const canChat = chatAvailable(lanes, choice?.agentId ?? null);
  const runAs: RunAs = canChat ? (runPick ?? defaultRunAs(lanes, choice?.agentId ?? null)) : 'terminal';
  const isolated = copyDefault(storedCopy) && copiesOk && runAs === 'terminal';

  const block = sendBlock({ prompt, choice, runAs, folder: folder?.path ?? null, busy });
  const example = exampleAt(exampleIdx);

  const send = async () => {
    if (block || !choice) return;
    setError(null);
    setBusy(true);
    remember(choice.key);
    try {
      await onStart({
        agentId: choice.agentId,
        accountId: choice.accountId,
        folder: folder?.path,
        isolated,
        runAs,
        prompt: prompt.trim(),
      });
    } catch (e) {
      if (alive.current) setError(e instanceof Error ? e.message : 'Could not start the agent.');
    } finally {
      if (alive.current) setBusy(false);
    }
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === 'Tab' && !e.shiftKey && !prompt) {
      e.preventDefault();
      setPrompt(example);
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  };

  const grow = (el: HTMLTextAreaElement) => {
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  };
  useEffect(() => {
    if (area.current) grow(area.current);
  }, [prompt]);

  const addFiles = async () => {
    setMenu(null);
    try {
      const paths = await api.pickPaths();
      if (paths.length > 0) setPrompt((text) => withPaths(text, paths));
    } catch {
      setError('Could not open the file dialog.');
    }
    area.current?.focus();
  };

  const chooseFolder = (project: ProjectChoice) => {
    setChosenFolder(project);
    setMenu(null);
    area.current?.focus();
  };
  const browseFolder = async () => {
    setError(null);
    try {
      const found = await api.pickFolder();
      if (found && found.state !== 'missing') chooseFolder({ path: found.path, name: found.name, state: found.state });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not open the folder dialog.');
    }
  };

  const setCards = (cards: HomeCard[]) => {
    setHome((h) => ({ ...h, cards }));
    api.saveCards(cards).catch(() => setError('Your cards were not saved.'));
  };

  const cardData: CardData = {
    recent: recent ?? [],
    running: panes.filter((p) => p.status === 'running'),
    needs,
  };

  const name = cleanName(home.name);
  return (
    <section className="ck-na" aria-label="New agent">
      <header className="ck-na-top">
        <span className="ck-na-top-title">Agent</span>
      </header>
      <div className="ck-na-scroll">
        <div className="ck-na-center">
          <div className="ck-na-mark" aria-hidden="true">
            <IconSpark />
          </div>
          <h1 className="ck-na-greet" data-testid="na-greeting">
            {greeting(name)}
          </h1>

          <div className="ck-na-composer" ref={menuRoot}>
            <div className="ck-na-field">
              <textarea
                ref={area}
                className="ck-na-input"
                aria-label="What should the agent do?"
                placeholder={example}
                rows={2}
                autoFocus
                value={prompt}
                disabled={busy}
                onChange={(e) => setPrompt(e.target.value)}
                onKeyDown={onKey}
              />
              {!prompt && (
                <span className="ck-na-tabhint" aria-hidden="true">
                  <kbd>Tab</kbd> to use
                </span>
              )}
            </div>

            <div className="ck-na-row">
              <Pop
                id="plus"
                open={menu === 'plus'}
                onToggle={() => setMenu(menu === 'plus' ? null : 'plus')}
                label="Add to your message"
                trigger={<IconPlus />}
                iconOnly
              >
                <button type="button" role="menuitem" className="ck-na-item" onClick={() => void addFiles()}>
                  <span>Add files…</span>
                  <span className="ck-na-item-hint">their paths go in your message</span>
                </button>
                <div className="ck-na-toggle" title="Work in a separate copy of the project, so agents never edit the same files. You review and merge it later.">
                  <span id="na-copy-label">Separate copy</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={isolated}
                    aria-labelledby="na-copy-label"
                    className="ck-switch"
                    disabled={!copiesOk || runAs === 'chat'}
                    onClick={() => {
                      setStoredCopy(!isolated);
                      writeCopyPreference(folderKey, !isolated);
                    }}
                  >
                    <span className="ck-switch-knob" />
                  </button>
                </div>
                {runAs === 'chat' ? (
                  <p className="ck-na-note">Chat sessions work in the folder itself.</p>
                ) : !copiesOk ? (
                  <p className="ck-na-note">This folder needs Git first.</p>
                ) : null}
              </Pop>

              <Pop
                id="folder"
                open={menu === 'folder'}
                onToggle={() => {
                  setMenu(menu === 'folder' ? null : 'folder');
                  api.recent().then((list) => alive.current && setRecent(list)).catch(() => undefined);
                }}
                label={`Folder: ${folder?.name ?? 'none chosen'}. Change`}
                trigger={
                  <>
                    <IconFolder />
                    <span className="ck-na-chip-text">{folder?.name ?? 'Choose a folder'}</span>
                    <IconChevronDown />
                  </>
                }
              >
                <div className="ck-na-cap">Recent projects</div>
                {(recent ?? []).filter((p) => p.state !== 'missing').map((p) => (
                  <button
                    key={p.path}
                    type="button"
                    role="menuitem"
                    className="ck-na-item"
                    title={p.path}
                    onClick={() => chooseFolder({ path: p.path, name: p.name, state: p.state })}
                  >
                    <span className="ck-na-item-main">
                      <span>{p.name}</span>
                      <span className="ck-na-item-path">{p.path}</span>
                    </span>
                    {folder?.path === p.path && <span className="ck-na-item-hint">in use</span>}
                  </button>
                ))}
                {recent !== null && recent.length === 0 && <p className="ck-na-note">No projects yet.</p>}
                <button type="button" role="menuitem" className="ck-na-item" onClick={() => void browseFolder()}>
                  <span>Choose folder…</span>
                </button>
              </Pop>

              <Pop
                id="agent"
                open={menu === 'agent'}
                onToggle={() => setMenu(menu === 'agent' ? null : 'agent')}
                label={`Agent: ${choice?.label ?? 'none'}. Change`}
                trigger={
                  <>
                    <span className={`ck-na-dot is-${choice?.state ?? 'missing'}`} aria-hidden="true" />
                    <span className="ck-na-chip-text">{choice?.label ?? 'No agent ready'}</span>
                    <IconChevronDown />
                  </>
                }
              >
                {choices.map((c) => (
                  <button
                    key={c.key}
                    type="button"
                    role="menuitemradio"
                    aria-checked={choice?.key === c.key}
                    className="ck-na-item"
                    disabled={c.state === 'unavailable'}
                    title={c.state === 'ready' ? undefined : c.hint}
                    onClick={() => {
                      setPickKey(c.key);
                      setRunPick(null);
                      setMenu(null);
                      area.current?.focus();
                    }}
                  >
                    <span className="ck-na-item-main">
                      <span className="ck-na-item-row">
                        <span className={`ck-na-dot is-${c.state}`} aria-hidden="true" />
                        <span>{c.label}</span>
                      </span>
                    </span>
                    <span className="ck-na-item-hint">{c.hint}</span>
                  </button>
                ))}
                {onSetUp && (
                  <button
                    type="button"
                    role="menuitem"
                    className="ck-na-item"
                    onClick={() => {
                      setMenu(null);
                      onSetUp();
                    }}
                  >
                    <span>Set up agents…</span>
                  </button>
                )}
              </Pop>

              {canChat && (
                <div className="ck-na-seg" role="radiogroup" aria-label="Run as">
                  {(['terminal', 'chat'] as const).map((mode) => (
                    <button
                      key={mode}
                      type="button"
                      role="radio"
                      aria-checked={runAs === mode}
                      className="ck-na-seg-btn"
                      onClick={() => setRunPick(mode)}
                    >
                      {mode === 'terminal' ? 'Terminal' : 'Chat'}
                    </button>
                  ))}
                </div>
              )}

              <span className="ck-na-spacer" />
              <button type="button" className="ck-na-send" aria-label="Send" title={block ?? 'Send (Enter)'} disabled={block !== null} onClick={() => void send()}>
                <IconArrowUp />
              </button>
            </div>
          </div>

          <p className="ck-na-hint" role={error ? 'alert' : undefined}>
            {error ?? (prompt.trim() && block ? block : '')}
          </p>

          <HomeCards
            cards={home.cards}
            data={cardData}
            onChange={setCards}
            onUseFolder={(p) => chooseFolder({ path: p.path, name: p.name, state: p.state })}
            onOpenPane={onOpenPane}
            onUsePrompt={(text) => {
              setPrompt(text);
              area.current?.focus();
            }}
          />
        </div>
      </div>
    </section>
  );
}

/** A chip that opens a small menu under it. */
function Pop({
  id,
  open,
  onToggle,
  label,
  trigger,
  iconOnly = false,
  children,
}: {
  id: string;
  open: boolean;
  onToggle: () => void;
  label: string;
  trigger: ReactNode;
  iconOnly?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="ck-na-pop">
      <button
        type="button"
        className={`ck-na-chip${iconOnly ? ' ck-na-chip--icon' : ''}`}
        data-testid={`na-${id}`}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={onToggle}
      >
        {trigger}
      </button>
      {open && (
        <div className="ck-na-menu" role="menu" aria-label={label}>
          {children}
        </div>
      )}
    </div>
  );
}
