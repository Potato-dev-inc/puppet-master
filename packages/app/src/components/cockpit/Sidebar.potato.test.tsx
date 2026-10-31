// @vitest-environment jsdom
// The sidebar's Potato Bot row: above the companion rows, with the unread count, only while the Labs switch is on.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { CompanionView } from '../../lib/companion';
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
const mochi = { id: 'mochi', name: 'Mochi', avatar: 'mochi', state: 'idle' } as CompanionView;

async function render(props: Partial<Parameters<typeof Sidebar>[0]> = {}) {
  await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} ship={ship} {...props} />));
  await act(async () => {});
}
const potatoRow = () => [...host.querySelectorAll<HTMLButtonElement>('button.ck-row')].find((b) => b.textContent?.includes('Potato Bot'));

describe('sidebar: Potato Bot', () => {
  it('has no row while the Labs switch is off, or before the app answered', async () => {
    await render({ potato: { enabled: false, unread: 3 }, onOpenPotato: () => {} });
    expect(potatoRow()).toBeUndefined();
    await render({ potato: null, onOpenPotato: () => {} });
    expect(potatoRow()).toBeUndefined();
    await render({});
    expect(potatoRow()).toBeUndefined();
  });

  it('shows the row with the unread count and opens the page', async () => {
    const onOpenPotato = vi.fn();
    await render({ potato: { enabled: true, unread: 3 }, onOpenPotato });
    const row = potatoRow()!;
    expect(row.querySelector('.ck-count')?.textContent).toBe('3');
    act(() => row.click());
    expect(onOpenPotato).toHaveBeenCalledTimes(1);
  });

  it('shows no count at zero, marks the row active while the page is open, and keeps the Inbox row quiet', async () => {
    await render({ potato: { enabled: true, unread: 0 }, potatoOpen: true, onOpenPotato: () => {} });
    const row = potatoRow()!;
    expect(row.querySelector('.ck-count')).toBeNull();
    expect(row.classList.contains('is-active')).toBe(true);
    const inbox = [...host.querySelectorAll<HTMLButtonElement>('button.ck-row')].find((b) => b.textContent?.includes('Inbox'))!;
    expect(inbox.classList.contains('is-active')).toBe(false);
  });

  it('sits above the companion rows', async () => {
    await render({ potato: { enabled: true, unread: 0 }, onOpenPotato: () => {}, companions: [mochi], onOpenCompanion: () => {} });
    const rows = [...host.querySelectorAll<HTMLElement>('nav.ck-side-scroll > button')];
    const at = (text: string) => rows.findIndex((r) => r.textContent?.includes(text));
    expect(at('Potato Bot')).toBeGreaterThanOrEqual(0);
    expect(at('Potato Bot')).toBeLessThan(at('Mochi'));
  });
});
