// @vitest-environment jsdom
// My computers: the page shell shows its title, its line and the empty state. The left bar row is shown only while the
// Labs switch (desk_enabled) is on; the flag helper is tested in lib/desk.test.ts.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DeskPage } from './DeskPage';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ShipApi } from '../../lib/ship-api';
import { Sidebar } from '../cockpit/Sidebar';

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

describe('DeskPage shell', () => {
  it('shows the title, the one line and the empty state', async () => {
    await act(async () => root.render(<DeskPage />));
    expect(host.querySelector('h2')?.textContent).toBe('My computers');
    expect(host.textContent).toContain('Share your mouse, keyboard and clipboard between your computers.');
    expect(host.textContent).toContain('Coming next: move between screens on this computer.');
  });
});

function fakeApi(): CockpitApi {
  return {
    registry: { paneList: [], panes: new Map(), initialReady: true },
    names: new Map(),
    asked: { questions: [] },
    launchEntries: [],
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
const deskRow = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-row')].find((b) => b.textContent?.includes('My computers'));

describe('My computers left bar row', () => {
  it('is hidden while the Labs switch is off', async () => {
    await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} ship={ship} desk={{ enabled: false }} onOpenDesk={() => {}} />));
    expect(deskRow()).toBeUndefined();
  });

  it('is shown when the Labs switch is on, and opens the page', async () => {
    const onOpenDesk = vi.fn();
    await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} ship={ship} desk={{ enabled: true }} deskOpen onOpenDesk={onOpenDesk} />));
    const row = deskRow();
    expect(row).toBeDefined();
    expect(row?.getAttribute('aria-current')).toBe('true');
    await act(async () => row?.click());
    expect(onOpenDesk).toHaveBeenCalledTimes(1);
  });
});
