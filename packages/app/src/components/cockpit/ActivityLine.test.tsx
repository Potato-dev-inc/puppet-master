// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { activityText, type ActivityMap } from '../../lib/pane-activity';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;

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

const pane = (id: string, status: string, cwd: string) => ({
  status,
  info: { id, agent_type: 'a', status, pid: 1, created_at: 1, last_output_at: null, cwd, cols: 80, rows: 24 },
});

function cockpit(activities: ActivityMap, status = 'running'): CockpitApi {
  const list = [pane('p1', status, 'C:\\work\\my-project')];
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([['p1', 'Agent A']]),
    activities,
    statusSince: new Map(),
    now: 0,
    selectedId: null,
    spawn: null,
    mode: 'orchestration',
    theme: 'dark',
    launchEntries: [],
    readyAgents: new Set(),
    bridgeLine: '',
    select: vi.fn(),
  } as unknown as CockpitApi;
}

const sub = () => host.querySelector('.ck-row:not(.ck-row--flat) .ck-row-sub')!;

describe('activityText', () => {
  it('shows the phrase only when it matches the pane status', () => {
    expect(activityText('running', { kind: 'working', text: 'Running npm test' })).toBe('Running npm test');
    expect(activityText('idle', { kind: 'working', text: 'Running npm test' })).toBeNull(); // stale by up to 1s
    expect(activityText('waiting_input', { kind: 'waiting', text: 'Waiting: allow `ls`?' })).toBe('Waiting: allow `ls`?');
    expect(activityText('running', undefined)).toBeNull();
    expect(activityText('running', { kind: 'working', text: '' })).toBeNull();
  });
});

describe('sidebar activity line', () => {
  it('replaces the project with the activity and keeps the project in the tooltip', () => {
    act(() => root.render(<Sidebar c={cockpit({ p1: { kind: 'working', text: 'Editing src/app.ts' } })} onOpenPalette={() => {}} />));
    expect(sub().textContent).toBe('Editing src/app.ts');
    expect(host.querySelector('.ck-row:not(.ck-row--flat) .ck-row-main')!.getAttribute('title')).toBe('my-project');
  });

  it('falls back to the project when there is no phrase yet', () => {
    act(() => root.render(<Sidebar c={cockpit({})} onOpenPalette={() => {}} />));
    expect(sub().textContent).toBe('my-project');
  });

  it('does not show a stale phrase after the status changed', () => {
    act(() =>
      root.render(<Sidebar c={cockpit({ p1: { kind: 'working', text: 'Running npm test' } }, 'idle')} onOpenPalette={() => {}} />),
    );
    expect(sub().textContent).toBe('my-project');
  });
});
