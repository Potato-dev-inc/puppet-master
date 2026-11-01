// @vitest-environment jsdom
// The Routines tab: the list (plain words, the next run, the pause switch), Run now, Delete (a question first), the
// New routine form (its values become the request), and what a failed call says in one sentence.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeRoutinesTransport, ROUTINE_COPY, type FakeRoutinesTransport, type RoutineView } from '../../../lib/potato-routines';
import { RoutinesTab } from './RoutinesTab';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Tuesday 2 November 2026, 8 am on the computer's clock.
const NOW = new Date(2026, 10, 2, 8, 0, 0).getTime();
const NEXT_MORNING = new Date(2026, 10, 3, 6, 0, 0).getTime();

const weekdays: RoutineView = {
  id: 'r-1',
  bot: 'nimbus',
  title: 'Morning sweep',
  prompt: 'Check my inbox.',
  schedule: { kind: 'weekdays', time: '06:00' },
  summary: 'Weekdays 6am',
  status: 'active',
  paused: false,
  done: false,
  lastRunMs: null,
  nextRunMs: NEXT_MORNING,
};
const hourly: RoutineView = {
  ...weekdays,
  id: 'r-2',
  title: 'Hourly check',
  schedule: { kind: 'hourly' },
  summary: 'Every hour',
  status: 'paused',
  paused: true,
  nextRunMs: null,
};
const someoneElses: RoutineView = { ...weekdays, id: 'r-3', bot: 'other', title: 'Not this bot' };

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

async function render(transport: FakeRoutinesTransport) {
  await act(async () => {
    root.render(<RoutinesTab bot={{ id: 'nimbus', name: 'Nimbus' }} transport={transport} now={NOW} />);
  });
  await flush();
}

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label);
const rows = () => [...host.querySelectorAll<HTMLElement>('.ck-potato-routine')];

/** Sets a field's value the way React hears it (a select changes on `change`). */
function setField(el: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  const proto =
    el instanceof HTMLSelectElement
      ? HTMLSelectElement.prototype
      : el instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(el, value);
  el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
}

const field = (selector: string) => host.querySelector<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(selector)!;

async function click(el: HTMLElement | undefined) {
  expect(el, 'button to click').toBeDefined();
  await act(async () => {
    el!.click();
  });
  await flush();
}

describe('Routines tab', () => {
  it('lists this bot routines in plain words, with the next run and a pause switch', async () => {
    const transport = fakeRoutinesTransport([weekdays, hourly, someoneElses]);
    await render(transport);
    expect(rows()).toHaveLength(2);
    expect(rows()[0].textContent).toContain('Morning sweep');
    expect(rows()[0].textContent).toContain('Weekdays 6am');
    expect(rows()[0].textContent).toContain('Next run tomorrow at 6:00');
    expect(rows()[0].querySelector('[role="switch"]')?.getAttribute('aria-checked')).toBe('true');
    expect(rows()[1].textContent).toContain('Paused');
    expect(rows()[1].querySelector('[role="switch"]')?.getAttribute('aria-checked')).toBe('false');
    expect(host.textContent).not.toContain('Not this bot');
  });

  it('says in one line that there are no routines yet', async () => {
    await render(fakeRoutinesTransport([]));
    expect(host.textContent).toContain(ROUTINE_COPY.empty);
    expect(rows()).toHaveLength(0);
  });

  it('the pause switch pauses and resumes the routine', async () => {
    const transport = fakeRoutinesTransport([weekdays]);
    await render(transport);
    await click(rows()[0].querySelector<HTMLButtonElement>('[role="switch"]') ?? undefined);
    expect(transport.calls.find((c) => c.method === 'pause')?.args).toEqual(['r-1', true]);
    expect(rows()[0].textContent).toContain('Paused');
    expect(rows()[0].querySelector('[role="switch"]')?.getAttribute('aria-checked')).toBe('false');
    await click(rows()[0].querySelector<HTMLButtonElement>('[role="switch"]') ?? undefined);
    expect(transport.calls.filter((c) => c.method === 'pause').at(-1)?.args).toEqual(['r-1', false]);
    expect(rows()[0].textContent).toContain('Next run');
  });

  it('Run now sends the prompt to the bot and says so', async () => {
    const transport = fakeRoutinesTransport([weekdays]);
    await render(transport);
    await click(button('Run now'));
    expect(transport.calls.find((c) => c.method === 'runNow')?.args).toEqual(['r-1']);
    expect(host.textContent).toContain('Sent to Nimbus.');
  });

  it('Delete asks first: Keep changes nothing, and the second Delete removes the row', async () => {
    const transport = fakeRoutinesTransport([weekdays]);
    await render(transport);
    await click(button('Delete'));
    expect(host.textContent).toContain('Delete this routine?');
    expect(transport.calls.some((c) => c.method === 'remove')).toBe(false);
    await click(button('Keep it'));
    expect(rows()).toHaveLength(1);
    await click(button('Delete'));
    await click(button('Delete'));
    expect(transport.calls.find((c) => c.method === 'remove')?.args).toEqual(['r-1']);
    expect(rows()).toHaveLength(0);
    expect(host.textContent).toContain(ROUTINE_COPY.empty);
  });

  it('the New routine form sends the values it was filled with, then shows the new row', async () => {
    const transport = fakeRoutinesTransport([]);
    await render(transport);
    await click(button(ROUTINE_COPY.newRoutine));
    const form = host.querySelector('form');
    expect(form).not.toBeNull();
    setField(field('input[type="text"]'), '  Morning sweep ');
    setField(field('textarea'), 'Check my inbox.');
    setField(field('select'), 'weekdays');
    setField(field('input[type="time"]'), '06:00');
    await act(async () => {
      form!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    });
    await flush();
    expect(transport.calls.find((c) => c.method === 'create')?.args).toEqual([
      'nimbus',
      { title: 'Morning sweep', prompt: 'Check my inbox.', schedule: { kind: 'weekdays', time: '06:00' } },
    ]);
    expect(host.querySelector('form')).toBeNull();
    expect(rows()).toHaveLength(1);
    expect(rows()[0].textContent).toContain('Morning sweep');
  });

  it('a blank name is refused on the screen, and nothing is sent', async () => {
    const transport = fakeRoutinesTransport([]);
    await render(transport);
    await click(button(ROUTINE_COPY.newRoutine));
    setField(field('textarea'), 'Check my inbox.');
    await click(button(ROUTINE_COPY.save));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Give the routine a name.');
    expect(transport.calls.some((c) => c.method === 'create')).toBe(false);
  });

  it('every few minutes below 15 is refused, and once takes a date and time', async () => {
    const transport = fakeRoutinesTransport([]);
    await render(transport);
    await click(button(ROUTINE_COPY.newRoutine));
    setField(field('input[type="text"]'), 'Quick look');
    setField(field('textarea'), 'Look at the queue.');
    setField(field('select'), 'every');
    await flush();
    setField(field('input[type="number"]'), '10');
    await click(button(ROUTINE_COPY.save));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Pick every 15 minutes or more.');

    setField(field('select'), 'once');
    await flush();
    setField(field('input[type="datetime-local"]'), '2026-11-05T09:30');
    await click(button(ROUTINE_COPY.save));
    expect(transport.calls.find((c) => c.method === 'create')?.args[1]).toEqual({
      title: 'Quick look',
      prompt: 'Look at the queue.',
      schedule: { kind: 'once', at: '2026-11-05T09:30' },
    });
  });

  it('a failed load says why in one plain sentence', async () => {
    await render(fakeRoutinesTransport([], 'POTATO_STORE_INVALID: routines.json is damaged.'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('routines.json is damaged.');
    expect(rows()).toHaveLength(0);
  });
});
