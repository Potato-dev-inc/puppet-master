// @vitest-environment jsdom
// The palette's Potato Bot entry: "Open Potato Bot" only when the page is reachable (the Labs switch is on).
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

function cockpit(): CockpitApi {
  return {
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
  } as unknown as CockpitApi;
}

const rowFor = (title: string) => [...host.querySelectorAll<HTMLElement>('[role="option"]')].find((r) => r.querySelector('.ck-palette-title')?.textContent === title);

async function open(props: Partial<ComponentProps<typeof Palette>> = {}) {
  await act(async () => root.render(<Palette c={cockpit()} onClose={() => {}} {...props} />));
}

describe('palette: Potato Bot', () => {
  it('has no entry while the Labs switch is off', async () => {
    await open();
    expect(rowFor('Open Potato Bot')).toBeUndefined();
    expect(host.textContent).not.toContain('Potato');
  });

  it('opens the page from "Open Potato Bot" while it is on', async () => {
    const onOpenPotato = vi.fn();
    await open({ onOpenPotato });
    const row = rowFor('Open Potato Bot');
    expect(row).toBeTruthy();
    act(() => row!.click());
    expect(onOpenPotato).toHaveBeenCalledTimes(1);
  });
});
