// @vitest-environment jsdom
// One row's second line (UX-8): each variant, and that the full row asks for its routines while the strip does not.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeRoutinesTransport } from '../../../lib/potato-routines';
import { BotListRow } from './BotListRow';
import { bot } from './test-helpers';

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
});

// Sunday 1 Nov 2026, 15:00.
const NOW = new Date(2026, 10, 1, 15, 0).getTime();
const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });

async function renderRow(view: Parameters<typeof bot>[0], routines = fakeRoutinesTransport([])) {
  act(() => {
    root.render(
      <BotListRow bot={bot(view)} avatar="mochi" active={false} mini={false} now={NOW} menu={false} menuOpen={false} onOpen={() => {}} onMenu={() => {}} routines={routines} />,
    );
  });
  await flush();
  return host.querySelector('.ck-potato-conv-status .ck-potato-status')?.textContent ?? null;
}

describe('BotListRow second line', () => {
  it('waiting on the person: Waiting for you, with the accent dot', async () => {
    const line = await renderRow({ id: 'n', name: 'Need', openCards: 1 });
    expect(line).toBe('Waiting for you');
    expect(host.querySelector<HTMLElement>('.ck-potato-status .ck-potato-dot')?.style.background).toBe('var(--accent)');
  });

  it('working: Thinking', async () => {
    expect(await renderRow({ id: 'w', name: 'Work', typing: true })).toBe('Thinking…');
  });

  it('paused: Paused', async () => {
    expect(await renderRow({ id: 'p', name: 'Pause', brainState: 'stopped' })).toBe('Paused');
  });

  it('idle with an active routine: Next, with its name and time', async () => {
    const routines = fakeRoutinesTransport([
      {
        id: 'r1', bot: 'idle', title: 'Morning sweep', prompt: 'x', schedule: { kind: 'daily', time: '09:00' }, summary: '',
        status: 'active', paused: false, done: false, lastRunMs: null, nextRunMs: new Date(2026, 10, 2, 9).getTime(),
      },
    ]);
    const line = await renderRow({ id: 'idle', name: 'Idle' }, routines);
    const time = new Date(2026, 10, 2, 9).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
    expect(line).toBe(`Next: Morning sweep · Tomorrow ${time}`);
    expect(routines.calls.map((c) => c.method)).toEqual(['list']);
  });

  it('idle with no routine: no second line, the preview stays', async () => {
    expect(await renderRow({ id: 'plain', name: 'Plain', lastLine: 'Hello there' })).toBeNull();
    expect(host.querySelector('.ck-potato-last')?.textContent).toBe('Hello there');
  });

  it('the strip asks for no routines', async () => {
    const routines = fakeRoutinesTransport([]);
    act(() => {
      root.render(
        <BotListRow bot={bot({ id: 's', name: 'S' })} avatar="mochi" active={false} mini menu={false} menuOpen={false} now={NOW} onOpen={() => {}} onMenu={() => {}} routines={routines} />,
      );
    });
    await flush();
    expect(routines.calls).toHaveLength(0);
  });
});
