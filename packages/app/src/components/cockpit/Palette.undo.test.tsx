// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { cockpitApi } from '../../lib/cockpit-api';
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
  registry: { paneList: [], panes: new Map([['p1', {}]]) },
  launchEntries: [],
  readyAgents: new Set(),
  names: new Map([['p1', 'Claude Code']]),
  selectedId: 'p1',
  mode: 'orchestration',
  theme: 'dark',
  interrupt: vi.fn(),
} as unknown as CockpitApi;

describe('Palette: Undo last turn', () => {
  it('asks the app to open the restore sheet and never restores silently', () => {
    const silent = vi.spyOn(cockpitApi, 'checkpointUndoLast');
    const onUndoLastTurn = vi.fn();
    act(() => root.render(<Palette c={c} onClose={vi.fn()} onUndoLastTurn={onUndoLastTurn} />));
    const row = Array.from(host.querySelectorAll('[role="option"]')).find((r) => r.textContent?.includes('Undo last turn'));
    expect(row).toBeTruthy();
    act(() => (row as HTMLElement).click());
    expect(onUndoLastTurn).toHaveBeenCalledExactlyOnceWith('p1');
    expect(silent).not.toHaveBeenCalled();
  });

  it('has no Undo row when the app cannot show the sheet', () => {
    act(() => root.render(<Palette c={c} onClose={vi.fn()} />));
    expect(host.textContent).not.toContain('Undo last turn');
  });
});
