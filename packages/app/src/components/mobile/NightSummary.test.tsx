// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { NightTaskView, NightView } from '../../lib/night';
import { NightSummary, bridgeNightApi, firstChars, ROW_TITLE_CHARS, TASK_ROWS_FIRST, type NightPhoneApi } from './NightSummary';

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
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** One task as `GET /night` returns it: the bridge view has no project, no pane id and no sentences. */
function task(overrides: Partial<NightTaskView> = {}): NightTaskView {
  return {
    id: 'nt-00000001',
    title: 'Fix the cart total rounding.',
    project: null,
    plan: 'Codex',
    state: 'finished',
    stateText: 'Finished',
    until: null,
    class: 'ready',
    headline: 'Tests pass 42 of 42',
    reason: null,
    tests: { passed: 42, total: 42 },
    testsBefore: { passed: 40, total: 42 },
    testChangeLines: [],
    files: { count: 5, added: 120, removed: 34 },
    turns: 3,
    duration: '38 min',
    summaries: [],
    attempts: [],
    review: 'pending',
    reviewText: null,
    into: null,
    canKeep: true,
    canThrowAway: true,
    paneId: null,
    ...overrides,
  };
}

/** A run as the bridge answers `GET /night`. */
function night(overrides: Partial<NightView> = {}): NightView {
  return {
    id: 'ns-0123456789ab',
    rev: 4,
    state: 'running',
    line: '1 of 3 done, tests green. 1 needs you.',
    total: 3,
    done: 1,
    green: 1,
    needsYou: 1,
    didntRun: 0,
    noChanges: 0,
    keepAll: 1,
    from: '23:10',
    to: null,
    turns: 3,
    duration: '38 min',
    byPlan: [{ label: 'Codex', tasks: 2 }],
    tasks: [
      task(),
      task({
        id: 'nt-00000002',
        title: 'Rename the config loader.',
        state: 'finished',
        stateText: 'Finished',
        class: 'needs_look',
        headline: 'Tests fail: 9 of 12 pass',
      }),
      task({
        id: 'nt-00000003',
        title: 'Fix broken links in /guides',
        state: 'queued',
        stateText: 'In the queue',
        class: null,
        headline: null,
        canKeep: false,
        canThrowAway: false,
      }),
    ],
    events: [],
    ...overrides,
  };
}

function fakeApi(read: NightView | null | Error = night(), stop: NightView | Error = night({ rev: 5 })): NightPhoneApi & {
  readMock: ReturnType<typeof vi.fn>;
  stopMock: ReturnType<typeof vi.fn>;
} {
  const readMock = vi.fn(async () => {
    if (read instanceof Error) throw read;
    return read;
  });
  const stopMock = vi.fn(async () => {
    if (stop instanceof Error) throw stop;
    return stop;
  });
  return { read: readMock, stop: stopMock, readMock, stopMock };
}

async function mount(api: NightPhoneApi | null, props: { canControl?: boolean; refreshKey?: number } = {}) {
  await act(async () => {
    root.render(<NightSummary api={api} canControl={props.canControl ?? true} refreshKey={props.refreshKey} />);
  });
}

async function click(el: Element | null | undefined) {
  await act(async () => {
    (el as HTMLButtonElement | null | undefined)?.click();
  });
}

const buttons = () => Array.from(host.querySelectorAll<HTMLButtonElement>('button'));
const buttonText = (text: string) => buttons().find((b) => b.textContent?.trim() === text);
const rows = () => Array.from(host.querySelectorAll('[aria-label="Night Shift tasks"] li'));

describe('NightSummary', () => {
  it('renders the summary line and one row per task from a GET /night fixture', async () => {
    const api = fakeApi(night());
    await mount(api);
    expect(api.readMock).toHaveBeenCalledTimes(1);
    expect(host.querySelector('.ph-card-q')?.textContent).toBe('1 of 3 done, tests green. 1 needs you.');
    expect(rows()).toHaveLength(3);
    const first = rows()[0];
    expect(first.querySelector('.ph-t')?.textContent).toBe('Fix the cart total rounding.');
    expect(first.querySelector('.ph-s')?.textContent).toBe('Tests pass 42 of 42');
    expect(first.querySelector('.ph-glyph')?.textContent).toBe('✓');
    expect(rows()[1].querySelector('.ph-glyph')?.textContent).toBe('!');
    // A task not classified yet shows its state, not an empty line.
    expect(rows()[2].querySelector('.ph-s')?.textContent).toBe('In the queue');
    expect(host.textContent).toContain('Keep and Throw away are on your computer.');
  });

  it('shows three task rows, then "and N more" that shows them all and folds back', async () => {
    const five = [1, 2, 3, 4, 5].map((n) => task({ id: `nt-0000000${n}`, title: `Task number ${n}` }));
    expect(TASK_ROWS_FIRST).toBe(3);
    await mount(fakeApi(night({ tasks: five })));
    expect(rows()).toHaveLength(TASK_ROWS_FIRST);
    expect(rows()[2].querySelector('.ph-t')?.textContent).toBe('Task number 3');
    expect(buttonText('and 2 more')).toBeDefined();

    await click(buttonText('and 2 more'));
    expect(rows()).toHaveLength(5);
    expect(rows()[4].querySelector('.ph-t')?.textContent).toBe('Task number 5');
    expect(buttonText('and 2 more')).toBeUndefined();

    await click(buttonText('Show fewer'));
    expect(rows()).toHaveLength(TASK_ROWS_FIRST);
    expect(buttonText('and 2 more')).toBeDefined();
  });

  it('shows no "more" button for three tasks or fewer', async () => {
    await mount(fakeApi(night()));
    expect(rows()).toHaveLength(3);
    expect(buttons().some((b) => /more$|Show fewer/.test(b.textContent ?? ''))).toBe(false);
  });

  it('the "more" button sends nothing to the PC', async () => {
    const api = fakeApi(night({ tasks: [1, 2, 3, 4].map((n) => task({ id: `nt-0000000${n}`, title: `Task ${n}` })) }));
    await mount(api);
    await click(buttonText('and 1 more'));
    expect(api.stopMock).not.toHaveBeenCalled();
    expect(api.readMock).toHaveBeenCalledTimes(1);
  });

  it('cuts a row title to 60 characters with an ellipsis', async () => {
    const long = 'x'.repeat(79) + '!';
    await mount(fakeApi(night({ tasks: [task({ title: long })] })));
    const shown = host.querySelector('.ph-t')?.textContent ?? '';
    expect(Array.from(shown)).toHaveLength(ROW_TITLE_CHARS);
    expect(shown.endsWith('…')).toBe(true);
    expect(firstChars('short', ROW_TITLE_CHARS)).toBe('short');
  });

  it('shows Stop only with the control cap, and only while the night runs', async () => {
    await mount(fakeApi(night()), { canControl: false });
    expect(buttonText('Stop Night Shift')).toBeUndefined();

    await act(async () => root.unmount());
    root = createRoot(host);
    await mount(fakeApi(night()), { canControl: true });
    expect(buttonText('Stop Night Shift')).toBeDefined();

    await act(async () => root.unmount());
    root = createRoot(host);
    await mount(fakeApi(night({ state: 'done', line: '3 of 3 done, tests green.' })), { canControl: true });
    expect(buttonText('Stop Night Shift')).toBeUndefined();
    expect(host.textContent).toContain('3 of 3 done, tests green.');
  });

  it('asks once more before it stops', async () => {
    const api = fakeApi(night());
    await mount(api);

    await click(buttonText('Stop Night Shift'));
    expect(api.stopMock).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Stop Night Shift now? Tasks still running are cut off. Their copies are kept.');

    // Not now: back to the plain button, nothing sent.
    await click(buttonText('Keep running'));
    expect(api.stopMock).not.toHaveBeenCalled();
    expect(buttonText('Stop Night Shift')).toBeDefined();

    await click(buttonText('Stop Night Shift'));
    await click(buttonText('Stop now'));
    expect(api.stopMock).toHaveBeenCalledTimes(1);
    // The PC stops at its next tick: the card says so and offers no second stop.
    expect(host.textContent).toContain('Stopping…');
    expect(buttonText('Stop Night Shift')).toBeUndefined();
  });

  it('shows only what the redacted view carries', async () => {
    // Fields the phone must never show, even if a view carried them: the card renders what it is given and nothing else.
    const leaky = night({
      events: [{ at: '23:12', kind: 'started', glyph: '~', text: 'SECRET EVENT TEXT', task: null }],
      tasks: [
        task({
          summaries: ['SECRET SENTENCE'],
          testChangeLines: ['SECRET TEST LINE'],
          attempts: [{ n: 1, label: 'Codex', end: 'completed', sentence: 'SECRET ATTEMPT', started: '23:10', ended: null }],
          reviewText: 'SECRET REVIEW',
          paneId: 'pane-42',
          project: 'SECRET-PROJECT',
        } as Partial<NightTaskView>),
      ],
    });
    await mount(fakeApi(leaky));
    const text = host.textContent ?? '';
    for (const secret of ['SECRET', 'pane-42', 'SECRET-PROJECT']) {
      expect(text).not.toContain(secret);
    }
  });

  it('renders nothing when no night has run, or when the PC refuses the read', async () => {
    await mount(fakeApi(null));
    expect(host.innerHTML).toBe('');

    await act(async () => root.unmount());
    root = createRoot(host);
    await mount(fakeApi(new Error('bridge GET /night -> 403')));
    expect(host.innerHTML).toBe('');
  });

  it('renders nothing without an api (no bridge yet)', async () => {
    await mount(null);
    expect(host.innerHTML).toBe('');
  });

  it('a failed stop says so and leaves the night running', async () => {
    const api = fakeApi(night(), new Error('bridge POST /night/stop -> 500'));
    await mount(api);
    await click(buttonText('Stop Night Shift'));
    await click(buttonText('Stop now'));
    expect(api.stopMock).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain('Could not stop Night Shift. Try again.');
    expect(buttonText('Stop Night Shift')).toBeDefined();
  });

  it('reads again when the stream resynced', async () => {
    const api = fakeApi(night());
    await mount(api, { refreshKey: 0 });
    expect(api.readMock).toHaveBeenCalledTimes(1);
    await mount(api, { refreshKey: 1 });
    expect(api.readMock).toHaveBeenCalledTimes(2);
  });

  it('reads every 10 seconds while it is up', async () => {
    vi.useFakeTimers();
    const api = fakeApi(night());
    await mount(api);
    expect(api.readMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(api.readMock).toHaveBeenCalledTimes(2);
  });
});

describe('bridgeNightApi', () => {
  const base = 'http://phone.test';

  it('reads GET /night and posts POST /night/stop, and maps 404 to no night', async () => {
    const payload = night();
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (url === `${base}/night` && (init?.method ?? 'GET') === 'GET') {
        return new Response(JSON.stringify(payload), { status: 200 });
      }
      if (url === `${base}/night/stop` && init?.method === 'POST') {
        return new Response(JSON.stringify({ ...payload, rev: 5 }), { status: 200 });
      }
      return new Response('{}', { status: 500 });
    });
    vi.stubGlobal('fetch', fetchMock);
    const api = bridgeNightApi(base);
    expect((await api.read())?.line).toBe(payload.line);
    expect((await api.stop()).rev).toBe(5);
    expect(fetchMock).toHaveBeenCalledTimes(2);

    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
    expect(await bridgeNightApi(base).read()).toBeNull();
  });

  it('throws when the PC refuses the read or the stop', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 403 })));
    await expect(bridgeNightApi(base).read()).rejects.toThrow('403');
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 409 })));
    await expect(bridgeNightApi(base).stop()).rejects.toThrow('409');
  });
});
