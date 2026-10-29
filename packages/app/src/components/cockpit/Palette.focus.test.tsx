// @vitest-environment jsdom
// The palette's Focus rows: the three lengths while off, End Focus and +15 min while on, the last used length
// first, and a row that starts Focus (SPEC n33 3.6 (3)). `invoke` is mocked.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { FocusView } from '../../lib/focus';
import { Palette } from './Palette';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
let focusView: FocusView;
const onClose = vi.fn();

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  window.localStorage.clear();
  focusView = OFF;
  invoke.mockImplementation(async (cmd: string) => (cmd === 'focus_state' ? focusView : null));
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  invoke.mockReset();
  onClose.mockReset();
  window.localStorage.clear();
});

function cockpit(): CockpitApi {
  const panes = [{ info: { id: 'p1', agent_type: 'codex', status: 'idle', cwd: 'C:\\work\\shop', title: null }, status: 'idle' }];
  return {
    registry: { paneList: panes, panes: new Map(panes.map((p) => [p.info.id, p])) },
    launchEntries: [],
    readyAgents: new Set(),
    names: new Map([['p1', 'Codex']]),
    selectedId: null,
    mode: 'orchestration',
    theme: 'dark',
    previews: {},
    openSettings: vi.fn(async () => {}),
    notify: vi.fn(),
  } as unknown as CockpitApi;
}

async function render() {
  await act(async () => root.render(<Palette c={cockpit()} onClose={onClose} />));
  await act(async () => {});
}

/** The titles of the Focus rows, in the order the palette shows them. */
const focusTitles = () =>
  [...host.querySelectorAll<HTMLElement>('.ck-palette-title')].map((t) => t.textContent ?? '').filter((t) => t.startsWith('Focus ') || t.includes('Focus'));

const row = (title: string) => [...host.querySelectorAll<HTMLElement>('[role="option"]')].find((r) => r.querySelector('.ck-palette-title')?.textContent === title);

describe('palette Focus rows while off', () => {
  it('lists the three lengths and no End Focus', async () => {
    await render();
    expect(focusTitles()).toEqual(['Focus 25 minutes', 'Focus 45 minutes', 'Focus 90 minutes']);
    expect(row('End Focus')).toBeUndefined();
  });

  it('lists the last used length first', async () => {
    window.localStorage.setItem('pm.focus.lastMinutes', '90');
    await render();
    expect(focusTitles()).toEqual(['Focus 90 minutes', 'Focus 25 minutes', 'Focus 45 minutes']);
  });

  it('a row starts Focus with that length and closes the palette', async () => {
    await render();
    await act(async () => row('Focus 90 minutes')!.click());
    await act(async () => {});
    expect(invoke).toHaveBeenCalledWith('focus_start', { minutes: 90 });
    expect(onClose).toHaveBeenCalled();
  });
});

describe('palette Focus rows while on', () => {
  beforeEach(() => {
    const now = Date.now();
    // One spare minute: the render happens a few milliseconds later, and "left" rounds down.
    focusView = { ...OFF, on: true, sessionId: 'fc-1', startedAtMs: now, endsAtMs: now + 32 * MIN, minutes: 45 };
  });

  it('lists End Focus with the time left, and Add 15 minutes to Focus', async () => {
    await render();
    expect(row('End Focus')).toBeDefined();
    expect(row('End Focus')!.querySelector('.ck-palette-desc')?.textContent).toBe('31 min left');
    expect(row('Add 15 minutes to Focus')).toBeDefined();
    expect(row('Focus 45 minutes')).toBeDefined();
  });

  it('End Focus calls focus_end', async () => {
    await render();
    await act(async () => row('End Focus')!.click());
    await act(async () => {});
    expect(invoke).toHaveBeenCalledWith('focus_end');
  });
});
