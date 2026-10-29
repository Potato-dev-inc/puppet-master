// @vitest-environment jsdom
import { act, type ComponentProps } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FindingView, JobView, Limits, Overview, WhatBrokeItChanged } from '../../lib/what-broke-it';
import { WhatBrokeItSheet, clampLimits, type WhatBrokeItApi } from './WhatBrokeItSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const OVERVIEW: Overview = {
  tests: [{ index: 0, name: 'test', command_text: 'npm test', timeout_min: 10, warnings: [] }],
  suggestions: [],
  points: [
    { checkpointId: '31', label: 'Codex · turn 7 · 14:10', atMs: 1 },
    { checkpointId: '28', label: 'Claude Code · turn 5 · 13:42', atMs: 2 },
  ],
  limits: { max_runs: 16, max_minutes: 20 },
  can: true,
  whyNot: null,
  busy: null,
};

const JOB: JobView = {
  id: 'wb-job-1',
  state: 'running',
  pointNo: 1,
  points: 9,
  run: 1,
  maxRuns: 16,
  elapsedMs: 0,
  maxMs: 1_200_000,
  line: 'Trying save point 1 of 9 · run 1 of 16 · 0 min',
  findingId: null,
};

const FINDING: FindingView = {
  id: 'wb-0123456789ab',
  title: 'Turn 7 by Codex broke the tests',
  subtitle: '14:10 · "refactor cart" · 4 files, +52 −20',
  kind: 'culprit',
  suspects: [],
  failing: ['cart.test.ts'],
  tail: '',
  canUndo: true,
  undoneAtMs: null,
};

/** A fake of the calls the sheet makes. `fire` plays a `what-broke-it://changed` event; `setStatus` sets what `wbStatus` answers. */
function mockApi(over: Partial<Record<keyof WhatBrokeItApi, unknown>> = {}) {
  let listener: ((e: WhatBrokeItChanged) => void) | null = null;
  const holder: { status: JobView } = { status: JOB };
  const api = {
    wbOverview: vi.fn(async () => OVERVIEW),
    wbDeclare: vi.fn(async (_pane: string, name: string, commandText: string, timeoutMin: number) => ({
      index: 0,
      name,
      command_text: commandText,
      timeout_min: timeoutMin,
      warnings: [],
    })),
    wbStart: vi.fn(async () => JOB),
    wbStatus: vi.fn(async () => holder.status),
    wbStop: vi.fn(async () => undefined),
    wbFinding: vi.fn(async () => FINDING),
    wbDiff: vi.fn(async () => ({ files: [], patch: null, truncated: false })),
    wbUndoPlan: vi.fn(async () => ({ writes: [], removes: [], conflicts: [], blockedBy: null })),
    wbUndo: vi.fn(async () => ({ undoneAtMs: 1, written: 0, removed: 0 })),
    wbRecheck: vi.fn(async () => JOB),
    onWhatBrokeItChanged: vi.fn(async (cb: (e: WhatBrokeItChanged) => void) => {
      listener = cb;
      return () => {
        listener = null;
      };
    }),
    ...over,
  };
  return {
    api: api as unknown as WhatBrokeItApi & typeof api,
    fire: (event: WhatBrokeItChanged) => listener?.(event),
    setStatus: (status: JobView) => {
      holder.status = status;
    },
  };
}

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

async function mount(
  api: WhatBrokeItApi,
  extra: Partial<ComponentProps<typeof WhatBrokeItSheet>> = {},
) {
  const notify = vi.fn();
  const onClose = vi.fn();
  await act(async () => {
    root.render(<WhatBrokeItSheet paneId="p1" notify={notify} onClose={onClose} api={api} {...extra} />);
  });
  return { notify, onClose };
}

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);
const findBtn = () => button('Find it') as HTMLButtonElement;
const limitsChange = () => Array.from(host.querySelectorAll<HTMLButtonElement>('.ck-wb-muted button')).find((b) => b.textContent === 'Change')!;
const alertText = () => host.querySelector('[role="alert"]')?.textContent ?? '';

/** Sets a field the way a person does: the native setter, then the event React listens to. */
async function typeInto(el: HTMLInputElement | HTMLSelectElement, value: string) {
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, 'value')!.set!;
  await act(async () => {
    setter.call(el, value);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  });
}

const input = (label: string) => host.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`) ?? null;
const numberInput = (name: string) =>
  Array.from(host.querySelectorAll('label'))
    .find((l) => l.textContent?.startsWith(name))
    ?.querySelector<HTMLInputElement>('input') ?? null;

describe('clampLimits', () => {
  it('keeps runs 1..30 and minutes 1..60', () => {
    expect(clampLimits({ max_runs: 99, max_minutes: 0 })).toEqual({ max_runs: 30, max_minutes: 1 });
    expect(clampLimits({ max_runs: -4, max_minutes: 500 })).toEqual({ max_runs: 1, max_minutes: 60 });
    expect(clampLimits({ max_runs: 16, max_minutes: 20 })).toEqual({ max_runs: 16, max_minutes: 20 });
  });
});

describe('WhatBrokeItSheet: set up', () => {
  it('shows the declared test, the save points and the limits, and starts with Find it for me', async () => {
    const { api } = mockApi();
    await mount(api);
    expect(host.querySelector('[role="dialog"]')!.textContent).toContain('What broke the tests?');
    expect(host.querySelector('[role="dialog"]')!.textContent).toContain('Your folder is not touched.');
    expect(host.querySelector('code')!.textContent).toBe('npm test');
    expect(host.textContent).toContain('Stops after 16 runs or 20 minutes.');
    expect(host.querySelector<HTMLSelectElement>('#ck-wb-good')!.value).toBe('');
    expect(findBtn().disabled).toBe(false);
  });

  it('with no declared test shows the suggestions, and Use this calls wbDeclare', async () => {
    const suggestion = { name: 'test', command_text: 'npm test', source: 'package.json' };
    const { api } = mockApi({
      wbOverview: vi.fn(async () => ({ ...OVERVIEW, tests: [], suggestions: [suggestion] })),
    });
    await mount(api);
    expect(host.textContent).toContain('None yet');
    expect(host.textContent).toContain('npm test');
    expect(host.textContent).toContain('package.json');
    expect(findBtn().disabled).toBe(true);
    await act(async () => button('Use this')!.click());
    expect(api.wbDeclare).toHaveBeenCalledExactlyOnceWith('p1', 'test', 'npm test', 10);
    expect(api.wbOverview).toHaveBeenCalledTimes(2);
  });

  it('a typed command with shell characters shows the backend message', async () => {
    const message = "Use a plain command. Shell characters such as && | > ; aren't allowed.";
    const { api } = mockApi({
      wbOverview: vi.fn(async () => ({ ...OVERVIEW, tests: [] })),
      wbDeclare: vi.fn(async () => {
        throw new Error(message);
      }),
    });
    await mount(api);
    await typeInto(input('Test command')!, 'npm test && rm -rf out');
    await act(async () => host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
    expect(api.wbDeclare).toHaveBeenCalledWith('p1', 'npm test && rm -rf out', 'npm test && rm -rf out', 10);
    expect(alertText()).toBe(message);
  });

  it('Find it is disabled and shows the reason when the backend says it cannot run', async () => {
    const { api } = mockApi({
      wbOverview: vi.fn(async () => ({ ...OVERVIEW, can: false, whyNot: 'A search is already running here.' })),
    });
    await mount(api);
    expect(findBtn().disabled).toBe(true);
    expect(host.textContent).toContain('A search is already running here.');
  });

  it('the limit fields clamp to the backend range and Find it sends the clamped values', async () => {
    const { api } = mockApi();
    await mount(api);
    await act(async () => limitsChange().click());
    await typeInto(numberInput('Runs')!, '99');
    await typeInto(numberInput('Minutes')!, '0');
    expect(numberInput('Runs')!.value).toBe('30');
    expect(numberInput('Minutes')!.value).toBe('1');
    await act(async () => findBtn().click());
    expect(api.wbStart).toHaveBeenCalledExactlyOnceWith('p1', 0, null, { max_runs: 30, max_minutes: 1 }, null);
  });

  it('a chosen save point is sent as the good checkpoint', async () => {
    const { api } = mockApi();
    await mount(api);
    await typeInto(host.querySelector<HTMLSelectElement>('#ck-wb-good')!, '28');
    await act(async () => findBtn().click());
    expect(api.wbStart).toHaveBeenCalledWith('p1', 0, '28', { max_runs: 16, max_minutes: 20 }, null);
  });

  it('shows the warnings of the declared test', async () => {
    const { api } = mockApi({
      wbOverview: vi.fn(async () => ({
        ...OVERVIEW,
        tests: [{ index: 0, name: 'test', command_text: 'npm test', timeout_min: 10, warnings: ['This command is slow.'] }],
      })),
    });
    await mount(api);
    expect(host.querySelector('.ck-wb-warn')!.textContent).toBe('This command is slow.');
  });

  it('names the other agents that work in this folder', async () => {
    const { api } = mockApi();
    await mount(api, { sharedWith: ['Codex 2'] });
    expect(host.textContent).toContain('Codex 2 also works in this folder. Its turns count too.');
  });

  it('a failed overview notifies and closes the sheet', async () => {
    const { api } = mockApi({ wbOverview: vi.fn(async () => Promise.reject(new Error('No folder for this pane.'))) });
    const { notify, onClose } = await mount(api);
    await act(async () => undefined);
    expect(notify).toHaveBeenCalledWith('No folder for this pane.');
    expect(onClose).toHaveBeenCalled();
  });
});

describe('WhatBrokeItSheet: running', () => {
  async function startSearch(api: ReturnType<typeof mockApi>['api']) {
    await mount(api);
    await act(async () => findBtn().click());
    await act(async () => undefined);
  }

  it('shows one status line from the job, the bar and Stop', async () => {
    const { api } = mockApi();
    await startSearch(api);
    expect(host.querySelector('.ck-wb-line')!.textContent).toBe('Trying save point 1 of 9 · run 1 of 16 · 0 min');
    expect(host.querySelector('.ck-wb-bar')).toBeTruthy();
    expect(host.textContent).toContain('Your agents can keep working. This runs at low priority.');
    expect(api.wbStart).toHaveBeenCalledTimes(1);
  });

  it('the progress line updates from events', async () => {
    const { api, fire, setStatus } = mockApi();
    await startSearch(api);
    setStatus({ ...JOB, pointNo: 4, run: 3, line: 'Trying save point 4 of 9 · run 3 of 16 · 2 min' });
    await act(async () => fire({ jobId: 'wb-job-1' }));
    expect(host.querySelector('.ck-wb-line')!.textContent).toBe('Trying save point 4 of 9 · run 3 of 16 · 2 min');
  });

  it('an event for another job is ignored', async () => {
    const { api, fire } = mockApi();
    await startSearch(api);
    const calls = () => (api.wbStatus as unknown as { mock: { calls: unknown[] } }).mock.calls.length;
    const before = calls();
    await act(async () => fire({ jobId: 'wb-other' }));
    expect(calls()).toBe(before);
  });

  it('Stop calls wbStop with the job id', async () => {
    const { api } = mockApi();
    await startSearch(api);
    await act(async () => button('Stop')!.click());
    expect(api.wbStop).toHaveBeenCalledExactlyOnceWith('wb-job-1');
  });

  it('Escape does not close a running search', async () => {
    const { api } = mockApi();
    const { onClose } = await mount(api);
    await act(async () => findBtn().click());
    await act(async () => undefined);
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    expect(onClose).not.toHaveBeenCalled();
    expect(host.querySelector('.ck-wb-line')).toBeTruthy();
  });

  it('a finished job renders the result component', async () => {
    const { api, fire, setStatus } = mockApi();
    await startSearch(api);
    setStatus({ ...JOB, state: 'done', findingId: FINDING.id });
    await act(async () => fire({ jobId: 'wb-job-1', findingId: FINDING.id }));
    expect(api.wbFinding).toHaveBeenCalledExactlyOnceWith(FINDING.id);
    expect(host.querySelector('.ck-wb-line')).toBeNull();
    expect(host.querySelector('.ck-wb-result h2')!.textContent).toBe(FINDING.title);
  });

  it('the default result card gets the sheet api, a Close button and is keyed by finding id', async () => {
    const { api, fire, setStatus } = mockApi();
    const { onClose } = await mount(api);
    await act(async () => findBtn().click());
    await act(async () => undefined);
    setStatus({ ...JOB, state: 'done', findingId: FINDING.id });
    await act(async () => fire({ jobId: 'wb-job-1', findingId: FINDING.id }));
    expect(button('Undo only this turn')).toBeTruthy();
    await act(async () => button('Show what it changed')!.click());
    expect(api.wbDiff).toHaveBeenCalledExactlyOnceWith(FINDING.id);
    await act(async () => button('Close')!.click());
    expect(onClose).toHaveBeenCalled();
  });

  it('Keep looking starts the same search again from the not-sure finding', async () => {
    const notSure: FindingView = { ...FINDING, id: 'wb-aaaaaaaaaaaa', kind: 'not_sure', title: 'Not sure. Ran out of runs.', canUndo: false };
    const { api, fire, setStatus } = mockApi({ wbFinding: vi.fn(async () => notSure) });
    await mount(api);
    await act(async () => findBtn().click());
    await act(async () => undefined);
    setStatus({ ...JOB, state: 'done', findingId: notSure.id });
    await act(async () => fire({ jobId: 'wb-job-1', findingId: notSure.id }));
    await act(async () => button('Keep looking')!.click());
    expect(api.wbStart).toHaveBeenLastCalledWith('p1', 0, null, { max_runs: 16, max_minutes: 20 }, notSure.id);
    expect(host.querySelector('.ck-wb-line')).toBeTruthy();
  });

  it('renders the result component passed in, with the finding', async () => {
    const { api, fire, setStatus } = mockApi();
    await mount(api, { renderResult: (f) => <p data-testid="result">Result for {f.title}</p> });
    await act(async () => findBtn().click());
    await act(async () => undefined);
    setStatus({ ...JOB, state: 'done', findingId: FINDING.id });
    await act(async () => fire({ jobId: 'wb-job-1', findingId: FINDING.id }));
    expect(host.querySelector('[data-testid="result"]')!.textContent).toBe(`Result for ${FINDING.title}`);
  });

  it('a stop that fails shows the backend message and keeps Stop', async () => {
    const { api } = mockApi({ wbStop: vi.fn(async () => Promise.reject(new Error('The search already ended.'))) });
    await startSearch(api);
    await act(async () => button('Stop')!.click());
    expect(alertText()).toBe('The search already ended.');
    expect(button('Stop')!.disabled).toBe(false);
  });
});
