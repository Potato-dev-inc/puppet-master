// @vitest-environment jsdom
// The Focus pill sits under the sidebar header: visible while Focus is on, absent while it is off.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { FocusView } from '../../lib/focus';
import type { ShipApi } from '../../lib/ship-api';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const MIN = 60_000;
const OFF: FocusView = {
  on: false,
  sessionId: null,
  startedAtMs: null,
  endsAtMs: null,
  minutes: 0,
  extended: 0,
  held: { run_done: 0, run_failed: 0, companion: 0, review: 0, night: 0 },
  heldTotal: 0,
  waitingQuiet: 0,
  waitingUrgent: 0,
  settings: { default_minutes: 45, quiet_blocking: true },
  lastSummary: null,
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
  invoke.mockReset();
});

function fakeApi(): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 80, rows: 24, title: null } as unknown as PaneInfo;
  const panes = [{ info, status: 'idle' }];
  return {
    registry: { paneList: panes, panes: new Map([['p1', panes[0]]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    asked: { questions: [] },
    launchEntries: [{ type: 'codex', label: 'Codex' }],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: 'p1',
    select: vi.fn(),
    prompts: {},
    statusSince: new Map([['p1', 0]]),
    now: 60_000,
    spawn: null,
    toasts: [],
    notify: vi.fn(),
    openSettings: vi.fn(async () => {}),
    bridgeLine: '',
  } as unknown as CockpitApi;
}

const ship = { recent: vi.fn().mockResolvedValue([]), gitState: vi.fn().mockResolvedValue('repo') } as unknown as ShipApi;

async function render() {
  await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} ship={ship} />));
  await act(async () => {});
}

describe('Sidebar Focus pill', () => {
  it('is absent while Focus is off', async () => {
    invoke.mockImplementation(async () => OFF);
    await render();
    expect(host.querySelector('[data-testid="focus-pill"]')).toBeNull();
    expect(host.querySelector('.ck-side')).not.toBeNull();
  });

  it('sits under the header while Focus is on', async () => {
    const now = Date.now();
    // One spare minute: the render happens a few milliseconds later, and "left" rounds down.
    const on: FocusView = { ...OFF, on: true, sessionId: 'fc-1', startedAtMs: now, endsAtMs: now + 32 * MIN, minutes: 45 };
    invoke.mockImplementation(async (cmd: string) => (cmd === 'focus_state' ? on : null));
    await render();
    expect(host.querySelector('.ck-side-head + .ck-focus [data-testid="focus-pill"]')).not.toBeNull();
    expect(host.querySelector('.ck-focus-text')?.textContent).toBe('Focus · 31 min left');
  });
});
