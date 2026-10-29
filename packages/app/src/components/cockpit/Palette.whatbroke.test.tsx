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
  registry: { paneList: [], panes: new Map([['p1', {}]]) },
  launchEntries: [],
  readyAgents: new Set(),
  names: new Map([['p1', 'Claude Code']]),
  selectedId: 'p1',
  mode: 'orchestration',
  theme: 'dark',
  interrupt: vi.fn(),
} as unknown as CockpitApi;

const findRow = () =>
  Array.from(host.querySelectorAll('[role="option"]')).find((r) => r.textContent?.includes('Find what broke the tests'));

describe('Palette: Find what broke the tests', () => {
  it('shows the item next to Undo last turn, with its plain description', () => {
    act(() => root.render(<Palette c={c} onClose={vi.fn()} onUndoLastTurn={vi.fn()} onWhatBroke={vi.fn()} />));
    const row = findRow();
    expect(row).toBeTruthy();
    expect(row!.textContent).toContain("Look back through the agents' turns.");
    const titles = Array.from(host.querySelectorAll('[role="option"]')).map((r) => r.textContent ?? '');
    const undoAt = titles.findIndex((t) => t.includes('Undo last turn'));
    const whatAt = titles.findIndex((t) => t.includes('Find what broke the tests'));
    expect(undoAt).toBeGreaterThanOrEqual(0);
    expect(whatAt).toBe(undoAt + 1);
  });

  it('runs the handler with the selected pane and closes the palette', () => {
    const onWhatBroke = vi.fn();
    const onClose = vi.fn();
    act(() => root.render(<Palette c={c} onClose={onClose} onWhatBroke={onWhatBroke} />));
    act(() => (findRow() as HTMLElement).click());
    expect(onClose).toHaveBeenCalledOnce();
    expect(onWhatBroke).toHaveBeenCalledExactlyOnceWith('p1');
  });

  it('has no row when the app cannot show the sheet', () => {
    act(() => root.render(<Palette c={c} onClose={vi.fn()} />));
    expect(host.textContent).not.toContain('Find what broke the tests');
  });
});
