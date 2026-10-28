// @vitest-environment jsdom
// Night Shift in the sidebar: the chip once, "Tonight…" in the "+" menu only while Labs is on, and the one sheet host.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ShipApi } from '../../lib/ship-api';
import { openTonight } from '../../lib/night-host';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const night = vi.hoisted(() => ({ enabled: false }));
vi.mock('../../lib/night', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/night')>()),
  useNightEnabled: () => night.enabled,
}));
vi.mock('./night/NightChip', async () => {
  const { createElement: h } = await import('react');
  return { NightChip: ({ onOpenMorning }: { onOpenMorning: () => void }) => h('button', { type: 'button', onClick: onOpenMorning }, 'NIGHT CHIP') };
});
vi.mock('./night/TonightHost', async () => {
  const { createElement: h } = await import('react');
  return { TonightHost: ({ onClose }: { onClose: () => void }) => h('div', null, 'TONIGHT SHEET', h('button', { type: 'button', onClick: onClose }, 'Close sheet')) };
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  night.enabled = false;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function fakeApi(): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 80, rows: 24, title: null } as unknown as PaneInfo;
  const panes = [{ info, status: 'idle' }];
  return {
    registry: { paneList: panes, panes: new Map([['p1', panes[0]]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    asked: { questions: [] },
    launchEntries: [{ type: 'codex', label: 'Codex' }],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: 'p1',
    select: vi.fn(),
    prompts: {},
    statusSince: new Map([['p1', 0]]),
    now: 60_000,
    spawn: null,
    toasts: [],
    notify: vi.fn(),
    openSettings: vi.fn(async () => {}),
    bridgeLine: '',
  } as unknown as CockpitApi;
}

const ship = { recent: vi.fn().mockResolvedValue([]), gitState: vi.fn().mockResolvedValue('repo') } as unknown as ShipApi;

async function render(onOpenMorning?: () => void) {
  await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} onOpenMorning={onOpenMorning} ship={ship} />));
  await act(async () => {});
}
async function openMenu() {
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="New agent"]')!.click());
  await act(async () => {});
}
const tonightItem = () => [...host.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((b) => b.textContent?.includes('Tonight'));

describe('sidebar: Night Shift', () => {
  it('draws the chip once, and Morning opens from it', async () => {
    const open = vi.fn();
    await render(open);
    const chips = [...host.querySelectorAll('button')].filter((b) => b.textContent === 'NIGHT CHIP');
    expect(chips).toHaveLength(1);
    act(() => chips[0].click());
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('draws no chip without an Open Morning handler', async () => {
    await render();
    expect(host.textContent).not.toContain('NIGHT CHIP');
  });

  it('has no "Tonight…" item while the Labs switch is off', async () => {
    await render(() => {});
    await openMenu();
    expect(tonightItem()).toBeUndefined();
  });

  it('has the "Tonight…" item while Labs is on; it closes the menu and opens the sheet', async () => {
    night.enabled = true;
    await render(() => {});
    await openMenu();
    const item = tonightItem()!;
    expect(item).toBeTruthy();
    await act(async () => item.click());
    expect(host.textContent).toContain('TONIGHT SHEET');
    expect(host.querySelector('.ck-menu--wide')).toBeNull();
    await act(async () => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Close sheet')!.click());
    expect(host.textContent).not.toContain('TONIGHT SHEET');
  });

  it('opens the sheet when the palette asks, only while Labs is on', async () => {
    await render(() => {});
    act(() => openTonight());
    expect(host.textContent).not.toContain('TONIGHT SHEET');
    night.enabled = true;
    await render(() => {});
    act(() => openTonight());
    expect(host.textContent).toContain('TONIGHT SHEET');
  });
});
