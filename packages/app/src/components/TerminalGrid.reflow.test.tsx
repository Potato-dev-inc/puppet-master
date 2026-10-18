// @vitest-environment jsdom
// The grid must leave a pane alone when something ELSE changes: another pane opening or closing (an
// agent run's pane, a warm standby), a status update, a fresh pane list. Only a pane that really came
// back from its own window plays the settle animation and gets a reflow, because every refit that
// changes the cell size resizes the PTY and makes an inline TUI (Claude Code and friends) reprint.
import { act, useEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneData, PaneRegistryApi } from '../hooks/usePaneRegistry';
import { TerminalGrid } from './TerminalGrid';

interface Seen {
  mounts: number;
  unmounts: number;
  reflowKey: unknown;
  redrawKey: unknown;
  keys: string[];
}
const seen = new Map<string, Seen>();

vi.mock('./TerminalPane', () => ({
  TerminalPane: (props: { pane: PaneData; reflowKey?: unknown; redrawKey?: unknown }) => {
    const id = props.pane.info.id;
    const entry = seen.get(id) ?? { mounts: 0, unmounts: 0, reflowKey: undefined, redrawKey: undefined, keys: [] };
    seen.set(id, entry);
    entry.reflowKey = props.reflowKey;
    entry.redrawKey = props.redrawKey;
    const key = `${String(props.reflowKey)}|${String(props.redrawKey)}`;
    if (entry.keys[entry.keys.length - 1] !== key) entry.keys.push(key);
    useEffect(() => {
      entry.mounts += 1;
      return () => {
        entry.unmounts += 1;
      };
    }, [entry]);
    return <div data-testid={`pane-${id}`} />;
  },
}));
vi.mock('../lib/detached-pane-window', () => ({
  detachedWindowSizeFromGrid: () => ({ width: 800, height: 600 }),
  openDetachedPaneWindow: vi.fn(async () => undefined),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function pane(id: string, status = 'idle'): PaneData {
  return {
    status: status as PaneData['status'],
    info: { id, agent_type: 'claude', status, pid: 1, created_at: 1, cwd: 'proj', cols: 100, rows: 30 } as PaneData['info'],
  };
}

function registry(list: PaneData[]): PaneRegistryApi {
  return {
    panes: new Map(list.map((p) => [p.info.id, p])),
    paneList: list,
    initialReady: true,
    spawnPane: vi.fn(),
    replacePaneAgent: vi.fn(),
    subscribePaneData: vi.fn(() => () => undefined),
  } as unknown as PaneRegistryApi;
}

let host: HTMLDivElement;
let root: Root;
const noop = () => undefined;

function render(list: PaneData[], detached: string[] = []) {
  return act(async () => {
    root.render(
      <TerminalGrid registry={registry(list)} projectPath="proj" onClosePane={noop} detachedPaneIds={new Set(detached)} />,
    );
  });
}
const cellOf = (id: string) => host.querySelector(`[data-testid="pane-${id}"]`)!.parentElement as HTMLElement;
const advance = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

beforeEach(() => {
  vi.useFakeTimers();
  seen.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('TerminalGrid leaves unaffected panes alone', () => {
  it('another pane opening does not settle, reflow or remount the existing pane', async () => {
    await render([pane('a')]);
    await advance(600);
    const before = [...seen.get('a')!.keys];

    await render([pane('a'), pane('b')]);
    expect(cellOf('a').className).not.toContain('pm-pane-settle');
    await advance(600);

    expect(seen.get('a')!.keys).toEqual(before);
    expect(seen.get('a')!.mounts).toBe(1);
    expect(seen.get('a')!.unmounts).toBe(0);
    expect(cellOf('a').className).not.toContain('pm-pane-settle');
  });

  it('another pane closing does not settle, reflow or remount the remaining pane', async () => {
    await render([pane('a'), pane('b')]);
    await advance(600);
    const before = [...seen.get('a')!.keys];

    await render([pane('a')]);
    expect(cellOf('a').className).not.toContain('pm-pane-settle');
    await advance(600);

    expect(seen.get('a')!.keys).toEqual(before);
    expect(seen.get('a')!.unmounts).toBe(0);
  });

  it('a status update or a fresh pane list does not remount or reflow', async () => {
    await render([pane('a'), pane('b')]);
    await advance(600);
    const before = [...seen.get('a')!.keys];

    await render([pane('a', 'running'), pane('b', 'waiting_input')]);
    await render([pane('a', 'idle'), pane('b', 'idle')]);
    await advance(600);

    expect(seen.get('a')!.keys).toEqual(before);
    expect(seen.get('a')!.mounts).toBe(1);
    expect(seen.get('a')!.unmounts).toBe(0);
  });

  it('a pane that comes back from its own window settles and reflows, its neighbours do not', async () => {
    await render([pane('a'), pane('b')], ['b']);
    await advance(600);
    const neighbour = [...seen.get('a')!.keys];
    expect(host.querySelector('[data-testid="pane-b"]')).toBeNull();

    await render([pane('a'), pane('b')], []);
    const returned = cellOf('b');
    expect(returned.className).toContain('pm-pane-settle');
    expect(returned.className).toContain('pm-pane-settle--shrink');
    expect(cellOf('a').className).not.toContain('pm-pane-settle');

    const keysAtReturn = [...seen.get('b')!.keys];
    await advance(600);
    // The animation ends, and a second reflow lands once it has finished.
    expect(returned.className).not.toContain('pm-pane-settle');
    expect(seen.get('b')!.keys.length).toBeGreaterThan(keysAtReturn.length);
    expect(seen.get('a')!.keys).toEqual(neighbour);
    expect(seen.get('a')!.unmounts).toBe(0);
  });

  it('a pane opening while another one is returning does not settle the newcomer or the bystander', async () => {
    await render([pane('a'), pane('b')], ['b']);
    await advance(600);
    await render([pane('a'), pane('b'), pane('c')], []);
    expect(cellOf('b').className).toContain('pm-pane-settle');
    expect(cellOf('a').className).not.toContain('pm-pane-settle');
    expect(cellOf('c').className).not.toContain('pm-pane-settle');
  });
});
