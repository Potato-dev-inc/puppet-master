// @vitest-environment jsdom
// Chat sessions (ACP) in the "+" menu of the sidebar: the Terminal | Chat switch shows only while Labs is on and the agent
// has an enabled chat definition; Chat starts a chat session in the menu's folder (the person's pick, so `explicit`) and
// selects the new pane; a failed start says why.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { AcpLaneView } from '../../lib/acpChat';
import { fakeAcpApi } from '../../lib/acpChatFake';
import type { ShipApi } from '../../lib/ship-api';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function cockpit() {
  const info = { id: 'p1', agent_type: 'codex', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 80, rows: 24, title: null } as unknown as PaneInfo;
  const panes = [{ info, status: 'idle' }];
  const select = vi.fn();
  const newAgent = vi.fn(async (_agent: string, ..._rest: unknown[]) => undefined);
  const c = {
    registry: { paneList: panes, panes: new Map([['p1', panes[0]]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    asked: { questions: [] },
    launchEntries: [{ type: 'codex', label: 'Codex' }, { type: 'claude', label: 'Claude' }],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: 'p1',
    select,
    newAgent,
    prompts: {},
    statusSince: new Map([['p1', 0]]),
    now: 60_000,
    spawn: null,
    toasts: [],
    notify: vi.fn(),
    openSettings: vi.fn(async () => {}),
    bridgeLine: '',
  } as unknown as CockpitApi;
  return { c, select, newAgent };
}

const ship = { recent: vi.fn().mockResolvedValue([]), gitState: vi.fn().mockResolvedValue('repo') } as unknown as ShipApi;
const lanes = (over: Partial<AcpLaneView> = {}): AcpLaneView => ({
  labs_enabled: true,
  agents: [{ id: 'codex', label: 'Codex', lane: 'terminal', effective_lane: 'terminal', disabled: false, disabled_note: null }],
  ...over,
});

async function openMenu(c: CockpitApi, acp: ReturnType<typeof fakeAcpApi>) {
  await act(async () => root.render(<Sidebar c={c} onOpenPalette={() => {}} ship={ship} acp={acp} />));
  await act(async () => {});
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="New agent"]')!.click());
  await act(async () => {});
}
const seg = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('.ck-lane-seg-btn')].find((b) => b.textContent === label)!;
const agentRow = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button.ck-menu-item')].find((b) => b.textContent?.startsWith(label))!;

describe('Sidebar "+" menu with chat sessions', () => {
  it('has no Terminal | Chat switch while Labs is off', async () => {
    const { c } = cockpit();
    await openMenu(c, fakeAcpApi({ laneGet: async () => lanes({ labs_enabled: false }) }));
    expect(host.querySelector('.ck-lane-seg')).toBeNull();
  });

  it('has the switch only on the agent with a chat definition', async () => {
    const { c } = cockpit();
    await openMenu(c, fakeAcpApi({ laneGet: async () => lanes() }));
    expect(host.querySelectorAll('.ck-lane-seg')).toHaveLength(1);
    expect(host.querySelector('[data-testid="lane-row-codex"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="lane-row-claude"]')).toBeNull();
  });

  it('Terminal (the default) starts the agent as before', async () => {
    const { c, newAgent } = cockpit();
    const start = vi.fn();
    await openMenu(c, fakeAcpApi({ laneGet: async () => lanes(), start }));
    await act(async () => agentRow('Codex').click());
    expect(newAgent).toHaveBeenCalledTimes(1);
    expect(newAgent.mock.calls[0][0]).toBe('codex');
    expect(start).not.toHaveBeenCalled();
  });

  it('Chat starts a chat session in the project folder as the person\'s own pick, and selects the new pane', async () => {
    const { c, select, newAgent } = cockpit();
    const start = vi.fn(async () => ({ pane_id: 'chat-9', agent: 'codex', lane: 'acp' as const }));
    await openMenu(c, fakeAcpApi({ laneGet: async () => lanes(), start }));
    await act(async () => seg('Chat').click());
    await act(async () => agentRow('Codex').click());
    await act(async () => {});
    expect(start).toHaveBeenCalledWith('codex', 'C:\\work\\shop', true);
    expect(select).toHaveBeenCalledWith('chat-9');
    expect(newAgent).not.toHaveBeenCalled();
    // The menu is closed.
    expect(host.querySelector('[role="menu"]')).toBeNull();
  });

  it('a chat that cannot start says why and offers the terminal', async () => {
    const { c, newAgent } = cockpit();
    const start = vi.fn(async () => {
      throw 'ACP_ADAPTER_MISSING: The chat could not start. The terminal can run this agent instead.';
    });
    await openMenu(c, fakeAcpApi({ laneGet: async () => lanes({ agents: [{ id: 'codex', label: 'Codex', lane: 'acp', effective_lane: 'acp', disabled: false, disabled_note: null }] }), start }));
    await act(async () => agentRow('Codex').click());
    await act(async () => {});
    expect(host.querySelector('[data-testid="chat-start-failed"]')?.textContent).toContain('The terminal can run this agent instead.');
    const fallback = [...host.querySelectorAll<HTMLButtonElement>('[data-testid="chat-start-failed"] button')].find((b) => b.textContent === 'Start in terminal')!;
    await act(async () => fallback.click());
    expect(newAgent.mock.calls[0][0]).toBe('codex');
  });
});
