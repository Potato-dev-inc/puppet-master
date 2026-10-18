// @vitest-environment jsdom
// While an agent drives a pane (status flips, prompts, ownership, a refreshed pane list every few
// hundred ms) the terminal component must neither remount nor reflow: a remount re-fits and resizes the
// PTY, and every PTY resize makes an inline TUI reprint. Only the explicit keys do anything.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneData, PaneRegistryApi } from '../hooks/usePaneRegistry';

const probe = vi.hoisted(() => ({
  constructed: 0,
  mounted: 0,
  disposed: 0,
  nudges: [] as Array<unknown>,
  held: [] as Array<[number, number]>,
}));

vi.mock('../terminal', () => ({
  TerminalSession: class {
    constructor() {
      probe.constructed += 1;
    }
    mount() {
      probe.mounted += 1;
    }
    dispose() {
      probe.disposed += 1;
    }
    nudgeReflow(options?: unknown) {
      probe.nudges.push(options ?? null);
      return true;
    }
    setReprintsOnResize() {}
    followHeldPty(cols: number, rows: number) {
      probe.held.push([cols, rows]);
    }
    afterReclaim() {}
    isScrolledUp() {
      return false;
    }
    getSelection() {
      return '';
    }
    setPtyDimensions() {}
    refreshFromSource() {}
    measureFitGrid() {
      return null;
    }
  },
}));
vi.mock('../lib/tauri', () => ({
  tauri: { resize: vi.fn(), writeInput: vi.fn(), reclaimPaneSize: vi.fn(async () => undefined) },
}));
vi.mock('../lib/agentRegistry', () => ({
  agentLabel: (_l: unknown, id: string) => id,
  useAgentRegistry: () => ({}),
}));

import { TerminalViewport } from './TerminalViewport';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function pane(over: Partial<PaneData['info']> = {}, status = 'idle'): PaneData {
  return {
    status: status as PaneData['status'],
    info: { id: 'p1', agent_type: 'claude', status, pid: 7, created_at: 11, last_output_at: null, cwd: 'proj', cols: 100, rows: 30, ...over } as PaneData['info'],
  };
}

const subscribePaneData = vi.fn(() => () => undefined);
const registry = { subscribePaneData } as unknown as PaneRegistryApi;

let host: HTMLDivElement;
let root: Root;
const frames = () => act(async () => { await vi.advanceTimersByTimeAsync(100); });

function render(p: PaneData, extra: { reflowKey?: number | string; redrawKey?: number | string; reg?: PaneRegistryApi } = {}) {
  return act(async () => {
    root.render(<TerminalViewport pane={p} registry={extra.reg ?? registry} chrome={false} reflowKey={extra.reflowKey} redrawKey={extra.redrawKey} />);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  probe.constructed = probe.mounted = probe.disposed = 0;
  probe.nudges.length = 0;
  probe.held.length = 0;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('TerminalViewport stays put while an agent drives the pane', () => {
  it('status, owner and pane-list refreshes neither remount nor reflow the terminal', async () => {
    await render(pane());
    await frames();
    expect(probe.constructed).toBe(1);

    await render(pane({}, 'running'));
    await render(pane({ last_output_at: 5 }, 'waiting_input'));
    await render(pane({ title: 'Doing the thing', last_output_at: 9 }, 'idle'));
    // A brand-new registry object with the same stream subscription (what every refresh makes).
    await render(pane({ last_output_at: 12 }), { reg: { ...registry } as PaneRegistryApi });
    await frames();

    expect(probe.constructed).toBe(1);
    expect(probe.disposed).toBe(0);
    expect(probe.nudges).toEqual([]);
  });

  it('a phone taking and releasing the pane follows the held size without remounting', async () => {
    await render(pane());
    await render(pane({ viewer_held: true, cols: 44, rows: 20 }));
    await render(pane({ viewer_held: false, cols: 100, rows: 30 }));
    await frames();
    expect(probe.constructed).toBe(1);
    expect(probe.disposed).toBe(0);
    expect(probe.nudges).toEqual([]);
    expect(probe.held).toEqual([[44, 20]]);
  });

  it('a plain reflow key is a cheap refit: no alternate-screen redraw', async () => {
    await render(pane(), { reflowKey: 'a' });
    await frames();
    probe.nudges.length = 0;
    await render(pane(), { reflowKey: 'b' });
    await frames();
    expect(probe.nudges).toEqual([null]);
    expect(probe.constructed).toBe(1);
  });

  it('the redraw key redraws only on a change, never for the value the pane mounts with', async () => {
    await render(pane(), { redrawKey: 3 });
    await frames();
    expect(probe.nudges).toEqual([]);
    await render(pane({ last_output_at: 1 }), { redrawKey: 3 });
    await frames();
    expect(probe.nudges).toEqual([]);
    await render(pane(), { redrawKey: 4 });
    await frames();
    expect(probe.nudges).toEqual([{ redrawAltBuffer: true }]);
    expect(probe.constructed).toBe(1);
  });

  it('a replaced process (new created_at) is the one thing that rebuilds the terminal', async () => {
    await render(pane());
    await render(pane({ created_at: 99 }));
    await frames();
    expect(probe.constructed).toBe(2);
    expect(probe.disposed).toBe(1);
  });
});
