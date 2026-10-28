// @vitest-environment jsdom
// The Focus note: off (three length chips), on (time left, held count, +15 min, End Focus), urgent (a
// plain count, not red), and the folded finished missions. Commands go through lib/focus.ts; invoke is mocked.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FocusView } from '../../lib/focus';
import { FocusNote } from './FocusNote';

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc.invoke }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_000_000_000;
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
const ON: FocusView = {
  ...OFF,
  on: true,
  sessionId: 's1',
  startedAtMs: NOW - 14 * 60_000,
  endsAtMs: NOW + 31 * 60_000 + 30_000,
  minutes: 45,
  held: { run_done: 2, run_failed: 0, companion: 1, review: 0, night: 0 },
  heldTotal: 3,
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  ipc.invoke.mockReset();
  ipc.invoke.mockResolvedValue(ON);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const render = (props: Partial<Parameters<typeof FocusNote>[0]> & { view: FocusView | null }) =>
  act(() => root.render(<FocusNote now={NOW} {...props} />));
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());
const flush = () => act(() => new Promise((r) => setTimeout(r, 0)));

describe('the note while Focus is off', () => {
  it('shows a Focus line and the three length chips, the default one marked', () => {
    render({ view: OFF });
    expect(host.querySelector('[data-focus="off"]')).not.toBeNull();
    expect(host.querySelector('.ck-focus-note-label')?.textContent).toBe('Focus');
    const chips = [...host.querySelectorAll('button')];
    expect(chips.map((c) => c.textContent)).toEqual(['25 min', '45 min', '90 min']);
    expect(chips.map((c) => c.getAttribute('data-default'))).toEqual([null, 'true', null]);
  });

  it('marks the default length the person set', () => {
    render({ view: { ...OFF, settings: { default_minutes: 90, quiet_blocking: true } } });
    const marked = [...host.querySelectorAll('[data-default="true"]')];
    expect(marked.map((c) => c.textContent)).toEqual(['90 min']);
  });

  it('a chip starts Focus with that length through focus_start', async () => {
    render({ view: OFF });
    click(button('25 min'));
    await flush();
    expect(ipc.invoke).toHaveBeenCalledWith('focus_start', { minutes: 25 });
  });

  it('hands the answer to onChange so the note shows the new state at once', async () => {
    const onChange = vi.fn();
    render({ view: OFF, onChange });
    click(button('90 min'));
    await flush();
    expect(onChange).toHaveBeenCalledWith(ON);
  });

  it('ignores the folded count (folding happens only while Focus is on)', () => {
    render({ view: OFF, foldedCount: 2 });
    expect(host.textContent).not.toContain('wait until Focus ends');
  });

  it('draws nothing without a view (not read yet, or no desktop shell)', () => {
    render({ view: null });
    expect(host.innerHTML).toBe('');
  });

  it('says so when the length could not be changed', async () => {
    ipc.invoke.mockRejectedValueOnce(new Error('refused'));
    render({ view: OFF });
    click(button('45 min'));
    await flush();
    expect(host.textContent).toContain("Couldn't change Focus. Try again.");
  });
});

describe('the note while Focus is on', () => {
  it('shows the time left, what is held, and the two buttons', () => {
    render({ view: ON });
    expect(host.querySelector('[data-focus="on"]')).not.toBeNull();
    expect(host.querySelector('.ck-focus-note-title')?.textContent).toBe('Focus · 31 min left');
    expect(host.textContent).toContain('Holding 3 updates. Things that need you still show.');
    expect(button('+15 min')).toBeDefined();
    expect(button('End Focus')).toBeDefined();
    expect(button('25 min')).toBeUndefined();
  });

  it('says nothing is held yet when nothing is', () => {
    render({ view: { ...ON, heldTotal: 0, held: { run_done: 0, run_failed: 0, companion: 0, review: 0, night: 0 } } });
    expect(host.textContent).toContain('Nothing held yet. Things that need you still show.');
  });

  it('+15 min calls focus_extend', async () => {
    render({ view: ON });
    click(button('+15 min'));
    await flush();
    expect(ipc.invoke).toHaveBeenCalledWith('focus_extend');
  });

  it('End Focus calls focus_end', async () => {
    render({ view: ON });
    click(button('End Focus'));
    await flush();
    expect(ipc.invoke).toHaveBeenCalledWith('focus_end');
  });

  it('shows a plain urgent count when a card needs the person now (no red class)', () => {
    render({ view: { ...ON, waitingUrgent: 1 } });
    const urgent = host.querySelector('.ck-focus-note-urgent');
    expect(urgent?.textContent).toBe('1 urgent');
    expect(urgent?.className).not.toMatch(/danger|red|error/);
  });

  it('shows no urgent count when none is waiting', () => {
    render({ view: { ...ON, waitingUrgent: 0 } });
    expect(host.querySelector('.ck-focus-note-urgent')).toBeNull();
  });

  it('under a minute reads "under 1 min left"', () => {
    render({ view: { ...ON, endsAtMs: NOW + 20_000 } });
    expect(host.querySelector('.ck-focus-note-title')?.textContent).toBe('Focus · under 1 min left');
  });
});

describe('the folded finished missions', () => {
  it('says how many wait until Focus ends, with a Show link', () => {
    render({ view: ON, foldedCount: 2 });
    expect(host.textContent).toContain('2 finished missions wait until Focus ends');
    expect(button('Show')).toBeDefined();
  });

  it('uses the singular for one mission', () => {
    render({ view: ON, foldedCount: 1 });
    expect(host.textContent).toContain('1 finished mission waits until Focus ends');
  });

  it('Show calls onShowFinished and shows no fold line when none are folded', () => {
    const onShowFinished = vi.fn();
    render({ view: ON, foldedCount: 2, onShowFinished });
    click(button('Show'));
    expect(onShowFinished).toHaveBeenCalledTimes(1);
    render({ view: ON, foldedCount: 0 });
    expect(host.textContent).not.toContain('wait until Focus ends');
  });
});
