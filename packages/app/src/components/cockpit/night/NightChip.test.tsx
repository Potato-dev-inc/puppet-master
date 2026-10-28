// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  EMPTY_STATE,
  LABS_OFF_STATE,
  MORNING_STATE,
  RUNNING,
  RUNNING_STATE,
  WAITING_STATE,
} from '../../../dev/night-fixtures';
import type { NightStateView } from '../../../lib/night';
import { chipView, NightChip, taskGlyph } from './NightChip';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));

/** The answers of the mocked `night_*` commands: the state on read, the stopped run on stop. */
function answer(state: NightStateView, stopped: NightStateView['run'] = null) {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'night_state') return Promise.resolve(state);
    if (cmd === 'night_stop') return Promise.resolve(stopped);
    return Promise.resolve(null);
  });
}

let host: HTMLDivElement;
let root: Root;

const setTauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

beforeEach(() => {
  setTauri(true);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  invoke.mockReset();
  setTauri(false);
});

async function render(state: NightStateView, onOpenMorning = vi.fn()) {
  answer(state);
  await act(async () => root.render(<NightChip onOpenMorning={onOpenMorning} />));
  return onOpenMorning;
}

const chip = () => host.querySelector('[data-testid="night-chip"]');
const popover = () => host.querySelector('[data-testid="night-popover"]');
const buttonByText = (text: string) =>
  [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === text) ?? null;
const click = async (el: Element | null) => {
  expect(el).not.toBeNull();
  await act(async () => (el as HTMLElement).click());
};

const SCHEDULED_STATE: NightStateView = {
  ...RUNNING_STATE,
  run: { ...RUNNING, state: 'scheduled', from: '23:30', line: '0 of 3 done' },
};

describe('NightChip states (SPEC 3.10.2)', () => {
  it('shows nothing when Labs is off', async () => {
    await render(LABS_OFF_STATE);
    expect(chip()).toBeNull();
  });

  it('shows nothing when there is no run yet', async () => {
    await render(EMPTY_STATE);
    expect(chip()).toBeNull();
  });

  it('running: name, count, the stop time, the dot spinner and Stop', async () => {
    await render(RUNNING_STATE);
    expect(chip()?.textContent).toContain('Night Shift');
    expect(chip()?.textContent).toContain('0 of 3 until 07:00');
    expect(host.querySelector('.pm-dots')).not.toBeNull();
    expect(buttonByText('Stop')).not.toBeNull();
    expect(chip()?.textContent).not.toContain('Open Morning');
  });

  it('waiting: says which plan and the time it may run again', async () => {
    await render(WAITING_STATE);
    expect(chip()?.textContent).toContain('Waiting for Codex until 01:10');
    expect(chip()?.textContent).not.toContain('until 07:00');
    expect(buttonByText('Stop')).not.toBeNull();
  });

  it('scheduled: says when it starts, and Stop is still there', async () => {
    await render(SCHEDULED_STATE);
    expect(chip()?.textContent).toContain('Starts 23:30');
    expect(buttonByText('Stop')).not.toBeNull();
  });

  it('finished: says how many are done and offers Open Morning, with no Stop', async () => {
    await render(MORNING_STATE);
    expect(chip()?.textContent).toContain('4 of 5 done');
    expect(chip()?.querySelector('.ck-night-open')?.textContent).toBe('Open Morning');
    expect(buttonByText('Stop')).toBeNull();
  });

  it('shows "+ N" when tasks need the person, and hides it at zero', async () => {
    await render(MORNING_STATE);
    expect(host.querySelector('.ck-night-more')?.textContent).toBe('+ 1');
    // The chip reads the state on mount, so the second state needs a fresh mount.
    await act(async () => root.unmount());
    root = createRoot(host);
    await render(RUNNING_STATE);
    expect(host.querySelector('.ck-night-more')).toBeNull();
  });
});

describe('NightChip actions', () => {
  it('Open Morning calls onOpenMorning once and stops nothing', async () => {
    const onOpenMorning = await render(MORNING_STATE);
    await click(chip()?.querySelector('.ck-night-chip-main') ?? null);
    expect(onOpenMorning).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalledWith('night_stop', undefined);
    expect(popover()).toBeNull();
  });

  it('a running chip opens a popover with each task, its plan and its state glyph', async () => {
    await render(RUNNING_STATE);
    await click(chip()?.querySelector('.ck-night-chip-main') ?? null);
    expect(popover()).not.toBeNull();
    const titles = [...host.querySelectorAll('.ck-night-task-title')].map((el) => el.textContent);
    expect(titles).toEqual(RUNNING.tasks.map((t) => t.title));
    expect(host.querySelectorAll('.ck-night-task-meta')[0]?.textContent).toBe('Codex');
    expect([...host.querySelectorAll('.ck-night-task .ck-night-glyph')].map((el) => el.textContent)).toEqual(['~', '-', '-']);
  });

  it('Escape closes the popover', async () => {
    await render(RUNNING_STATE);
    await click(chip()?.querySelector('.ck-night-chip-main') ?? null);
    await act(async () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })));
    expect(popover()).toBeNull();
  });

  it('Stop asks first: the confirm line shows, and nothing is stopped until the second press', async () => {
    await render(RUNNING_STATE);
    await click(buttonByText('Stop'));
    expect(popover()?.textContent).toContain('Stops everything now. Copies are kept.');
    expect(invoke).not.toHaveBeenCalledWith('night_stop', undefined);

    await click(buttonByText('Stop Night Shift'));
    expect(invoke).toHaveBeenCalledWith('night_stop', undefined);
  });

  it('Cancel in the confirm closes the question and stops nothing', async () => {
    await render(RUNNING_STATE);
    await click(buttonByText('Stop'));
    await click(buttonByText('Cancel'));
    expect(popover()?.textContent).not.toContain('Stops everything now.');
    expect(invoke).not.toHaveBeenCalledWith('night_stop', undefined);
  });

  it('the Stop in the popover asks the same way', async () => {
    await render(RUNNING_STATE);
    await click(chip()?.querySelector('.ck-night-chip-main') ?? null);
    await click(buttonByText('Stop Night Shift'));
    expect(popover()?.textContent).toContain('Stops everything now. Copies are kept.');
    expect(invoke).not.toHaveBeenCalledWith('night_stop', undefined);
  });

  it('a failed stop shows the plain sentence and keeps the popover open', async () => {
    await render(RUNNING_STATE);
    invoke.mockImplementation((cmd: string) =>
      cmd === 'night_state'
        ? Promise.resolve(RUNNING_STATE)
        : Promise.reject({ code: 'NIGHT_STOP_FAILED', message: 'Could not stop Night Shift. Try again.', recoverable: true, context: null }),
    );
    await click(buttonByText('Stop'));
    await click(buttonByText('Stop Night Shift'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Could not stop Night Shift. Try again.');
    expect(popover()).not.toBeNull();
  });
});

describe('chipView and taskGlyph (pure)', () => {
  it('hides for Labs off and for no run', () => {
    expect(chipView(LABS_OFF_STATE)).toBeNull();
    expect(chipView(EMPTY_STATE)).toBeNull();
  });

  it('reads each state from the run and the stop time', () => {
    expect(chipView(RUNNING_STATE)).toMatchObject({ kind: 'running', text: '0 of 3 until 07:00', canStop: true });
    expect(chipView(WAITING_STATE)).toMatchObject({ kind: 'waiting', text: 'Waiting for Codex until 01:10' });
    expect(chipView(SCHEDULED_STATE)).toMatchObject({ kind: 'scheduled', text: 'Starts 23:30' });
    expect(chipView(MORNING_STATE)).toMatchObject({ kind: 'finished', text: '4 of 5 done', needsYou: 1, canStop: false });
  });

  it('the task glyph follows the state and then the class', () => {
    const task = RUNNING.tasks[0];
    expect(taskGlyph({ ...task, state: 'working' })).toBe('~');
    expect(taskGlyph({ ...task, state: 'waiting' })).toBe(':');
    expect(taskGlyph({ ...task, state: 'finished', class: 'ready' })).toBe('v');
    expect(taskGlyph({ ...task, state: 'finished', class: 'needs_look' })).toBe('+');
    expect(taskGlyph({ ...task, state: 'finished', class: 'didnt_run' })).toBe('x');
  });
});
