// @vitest-environment jsdom
// "Keyboard shortcuts" is a row in the Ctrl+K palette, and choosing it opens the sheet.
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
    select: vi.fn(),
  } as unknown as CockpitApi;
}

describe('Palette: Keyboard shortcuts', () => {
  it('has a "Keyboard shortcuts" row that opens the sheet and closes the palette', () => {
    const onOpenShortcuts = vi.fn();
    const onClose = vi.fn();
    act(() => root.render(<Palette c={cockpit()} onClose={onClose} onOpenShortcuts={onOpenShortcuts} />));
    const row = [...host.querySelectorAll<HTMLElement>('[role="option"]')].find((r) => r.querySelector('.ck-palette-title')?.textContent === 'Keyboard shortcuts');
    expect(row).toBeDefined();
    act(() => row!.click());
    expect(onClose).toHaveBeenCalled();
    expect(onOpenShortcuts).toHaveBeenCalledTimes(1);
  });
});
