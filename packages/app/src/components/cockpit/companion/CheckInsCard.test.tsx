// @vitest-environment jsdom
// "Check-ins": the list when the view carries it, the next one only when it does not, and the add sheet that builds
// each `When` preset.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CheckIn, type CompanionView } from '../../../lib/companion';
import { CheckInsCard } from './CheckInsCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
  vi.restoreAllMocks();
});

const morning: CheckIn = {
  id: '0a1b2c3d',
  label: 'Morning brief',
  text: 'What is on today?',
  when: { kind: 'daily', at: { h: 8, m: 0 } },
  enabled: true,
};

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;

function setValue(el: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, 'value')!.set!;
  act(() => {
    setter.call(el, value);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  });
}

async function render(checkins: CheckIn[] | undefined, next: { id: string; label: string; atMs: number } | null = null) {
  const transport = fakeCompanionTransport();
  const onChanged = vi.fn<(v: CompanionView) => void>();
  await act(async () =>
    root.render(<CheckInsCard companionId="mochi" checkins={checkins} nextCheckin={next} transport={transport} onChanged={onChanged} />),
  );
  return { transport, onChanged };
}

/** Opens the add sheet, fills it, and presses Add. Returns the `When` that was saved. */
async function addWith(transport: ReturnType<typeof fakeCompanionTransport>, fill: () => void) {
  act(() => button('Add a check-in').click());
  fill();
  await act(async () => button('Add').click());
  const call = transport.calls.find((c) => c.method === 'setCheckIns');
  const list = call?.args[1] as CheckIn[];
  return list[list.length - 1].when;
}

describe('CheckInsCard', () => {
  it('shows only the next check-in, with no add button, while the view does not carry the list', async () => {
    await render(undefined, { id: '0a1b2c3d', label: 'Morning brief', atMs: 1 });
    expect(host.textContent).toContain('Next: Morning brief');
    expect(button('Add a check-in')).toBeUndefined();
  });

  it('lists each check-in with its schedule in plain words, and adds one', async () => {
    await render([morning]);
    expect(host.textContent).toContain('Morning brief');
    expect(host.textContent).toContain('Every day at 8:00');
    expect(button('Add a check-in')).toBeTruthy();
  });

  it('keeps the add button off until there is something to ask', async () => {
    await render([]);
    act(() => button('Add a check-in').click());
    expect(button('Add').disabled).toBe(true);
  });

  it('builds the "Every day" preset, with the time typed in', async () => {
    const { transport } = await render([]);
    const when = await addWith(transport, () => {
      setValue(host.querySelector<HTMLInputElement>('input[type="time"]')!, '07:30');
      setValue(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Morning brief please');
    });
    expect(when).toEqual({ kind: 'daily', at: { h: 7, m: 30 } });
  });

  it('builds "Weekdays" and "Once a week" with the day picked', async () => {
    const { transport } = await render([]);
    const weekdays = await addWith(transport, () => {
      setValue(host.querySelector<HTMLSelectElement>('select')!, 'weekdays');
      setValue(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Standup notes');
    });
    expect(weekdays).toEqual({ kind: 'weekdays', at: { h: 8, m: 0 } });
  });

  it('builds "Once a week" on the day picked', async () => {
    const { transport } = await render([]);
    const weekly = await addWith(transport, () => {
      setValue(host.querySelector<HTMLSelectElement>('select')!, 'weekly');
      setValue(host.querySelector<HTMLSelectElement>('select[aria-label="Day"]')!, '2');
      setValue(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Review the week');
    });
    expect(weekly).toEqual({ kind: 'weekly', day: 2, at: { h: 8, m: 0 } });
  });

  it('builds "Every N hours" in minutes', async () => {
    const { transport } = await render([]);
    const every = await addWith(transport, () => {
      setValue(host.querySelector<HTMLSelectElement>('select')!, 'every');
      setValue(host.querySelector<HTMLInputElement>('input[type="number"]')!, '4');
      setValue(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Check the build');
    });
    expect(every).toEqual({ kind: 'every', minutes: 240 });
  });

  it('builds "Once" from the date and time', async () => {
    const { transport } = await render([]);
    const once = await addWith(transport, () => {
      setValue(host.querySelector<HTMLSelectElement>('select')!, 'once');
      setValue(host.querySelector<HTMLInputElement>('input[type="datetime-local"]')!, '2026-11-01T09:30');
      setValue(host.querySelector<HTMLTextAreaElement>('textarea')!, 'Ask about the trip');
    });
    expect(once).toEqual({ kind: 'once', atMs: new Date('2026-11-01T09:30').getTime() });
  });
});
