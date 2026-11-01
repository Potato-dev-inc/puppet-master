// @vitest-environment jsdom
// The sidebar's "New agent" row: first in the list, opens the New agent page, active while it is open.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
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

function fakeApi(): CockpitApi {
  return {
    registry: { paneList: [], panes: new Map(), initialReady: true },
    names: new Map(),
    asked: { questions: [] },
    launchEntries: [{ type: 'codex', label: 'Codex' }],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: null,
    select: vi.fn(),
    prompts: {},
    statusSince: new Map(),
    now: 60_000,
    spawn: null,
    toasts: [],
    notify: vi.fn(),
    openSettings: vi.fn(async () => {}),
    bridgeLine: '',
  } as unknown as CockpitApi;
}

const ship = { recent: vi.fn().mockResolvedValue([]), gitState: vi.fn().mockResolvedValue('repo') } as unknown as ShipApi;

async function render(props: Partial<Parameters<typeof Sidebar>[0]> = {}) {
  await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} ship={ship} {...props} />));
  await act(async () => {});
}
const row = () => host.querySelector<HTMLButtonElement>('[data-testid="side-new-agent"]');
const inboxRow = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-row')].find((b) => b.textContent?.includes('Inbox'))!;

describe('sidebar: New agent row', () => {
  it('is not drawn without a handler', async () => {
    await render();
    expect(row()).toBeNull();
  });

  it('is the first row, says New agent, and opens the page', async () => {
    const onOpenNewAgent = vi.fn();
    await render({ onOpenNewAgent });
    expect(row()!.textContent).toBe('New agent');
    expect(host.querySelector('nav')!.firstElementChild).toBe(row());
    expect(row()!.classList.contains('ck-row')).toBe(false); // the pane rows and the Inbox row are found by that class
    act(() => row()!.click());
    expect(onOpenNewAgent).toHaveBeenCalledTimes(1);
  });

  it('is the active row while the page is open, and the Inbox row is then not', async () => {
    await render({ onOpenNewAgent: () => {}, newAgentOpen: false });
    expect(row()!.classList.contains('is-active')).toBe(false);
    expect(inboxRow().classList.contains('is-active')).toBe(true);
    await render({ onOpenNewAgent: () => {}, newAgentOpen: true });
    expect(row()!.classList.contains('is-active')).toBe(true);
    expect(row()!.getAttribute('aria-current')).toBe('page');
    expect(inboxRow().classList.contains('is-active')).toBe(false);
  });

  it('does not take the New agent button’s name (the + menu keeps its aria-label)', async () => {
    await render({ onOpenNewAgent: () => {} });
    expect(host.querySelectorAll('button[aria-label="New agent"]')).toHaveLength(1);
    expect(row()!.getAttribute('aria-label')).toBeNull();
  });
});
