// @vitest-environment jsdom
// The palette's "Start crew…" row: present when the app can open the sheet, found by "crew" or "team", closes the palette and opens the sheet.
import { act, type ComponentProps } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { Palette } from './Palette';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const cockpit = () =>
  ({
    registry: { paneList: [], panes: new Map() },
    launchEntries: [],
    readyAgents: new Set(),
    names: new Map(),
    selectedId: null,
    mode: 'orchestration',
    theme: 'dark',
    previews: {},
    openSettings: vi.fn(async () => {}),
    notify: vi.fn(),
  }) as unknown as CockpitApi;

const rows = () => [...host.querySelectorAll<HTMLElement>('[role="option"]')];
const crewRow = () => rows().find((r) => r.querySelector('.ck-palette-title')?.textContent === 'Start crew…');

async function open(props: Partial<ComponentProps<typeof Palette>> = {}) {
  await act(async () => root.render(<Palette c={cockpit()} onClose={() => {}} {...props} />));
}

function type(text: string) {
  const input = host.querySelector<HTMLInputElement>('.ck-palette-input')!;
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(input, text);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('palette: crews', () => {
  it('shows "Start crew…" and opens the sheet after closing the palette', async () => {
    const onStartCrew = vi.fn();
    const onClose = vi.fn();
    await open({ onStartCrew, onClose });
    expect(crewRow()).toBeTruthy();
    act(() => crewRow()!.click());
    expect(onClose).toHaveBeenCalled();
    expect(onStartCrew).toHaveBeenCalledTimes(1);
  });

  it('is found by the words crew and team', async () => {
    await open({ onStartCrew: vi.fn() });
    type('crew');
    expect(crewRow()).toBeTruthy();
    type('team');
    expect(crewRow()).toBeTruthy();
    type('zzzz');
    expect(crewRow()).toBeUndefined();
  });

  it('has no row when the app cannot open the sheet', async () => {
    await open();
    expect(crewRow()).toBeUndefined();
  });
});
