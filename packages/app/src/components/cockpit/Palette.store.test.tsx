// @vitest-environment jsdom
import { act } from 'react';
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
});

const c = {
  registry: { paneList: [], panes: new Map() },
  launchEntries: [],
  readyAgents: new Set(),
  names: new Map(),
  selectedId: null,
  mode: 'orchestration',
  theme: 'dark',
  interrupt: vi.fn(),
} as unknown as CockpitApi;

const row = () => Array.from(host.querySelectorAll('[role="option"]')).find((r) => r.textContent?.includes('Open Agent Store'));

describe('Palette: Open Agent Store', () => {
  it('opens the store from the command row', () => {
    const onOpenStore = vi.fn();
    act(() => root.render(<Palette c={c} onClose={vi.fn()} onOpenStore={onOpenStore} />));
    expect(row()).toBeTruthy();
    act(() => (row() as HTMLElement).click());
    expect(onOpenStore).toHaveBeenCalledTimes(1);
  });

  it('has no row when the app cannot show the store', () => {
    act(() => root.render(<Palette c={c} onClose={vi.fn()} />));
    expect(row()).toBeUndefined();
  });
});
