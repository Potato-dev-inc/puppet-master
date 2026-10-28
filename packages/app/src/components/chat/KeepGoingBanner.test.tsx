// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { keepGoingBrief, keepGoingLineage, onKeepGoingChanged, type HandoverView, type KeepGoingLineage } from '../../lib/keep-going';
import { KeepGoingBanner, pickHandover, plainKeepGoingError } from './KeepGoingBanner';

vi.mock('../../lib/keep-going', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/keep-going')>();
  return { ...actual, keepGoingLineage: vi.fn(), keepGoingBrief: vi.fn(), onKeepGoingChanged: vi.fn() };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Fake labels only. The banner shows whatever label the record carries.
const view = (patch: Partial<HandoverView> = {}): HandoverView => ({
  id: 'kg-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake B · Work',
  fromPaneId: 'pane-a',
  toPaneId: 'pane-b',
  atMs: new Date(2026, 9, 28, 17, 2).getTime(),
  state: 'running',
  stateText: 'Continued on Fake B · Work',
  turns: 14,
  briefBytes: 321,
  backAtMs: null,
  reason: 'usage_limit',
  ...patch,
});

const lineageMock = keepGoingLineage as Mock;
const briefMock = keepGoingBrief as Mock;
const listenMock = onKeepGoingChanged as Mock;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  lineageMock.mockReset();
  briefMock.mockReset();
  listenMock.mockReset();
  listenMock.mockResolvedValue(() => {});
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function mount(paneId: string, lineage: KeepGoingLineage | Error, onOpenPane?: (paneId: string) => void) {
  if (lineage instanceof Error) lineageMock.mockRejectedValue(lineage);
  else lineageMock.mockResolvedValue(lineage);
  await act(async () => {
    root.render(<KeepGoingBanner paneId={paneId} onOpenPane={onOpenPane} />);
  });
}
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);
const row = () => host.querySelector<HTMLElement>('[data-keep-going]');

describe('KeepGoingBanner: old pane (continued on)', () => {
  it('shows the server state line, the time and Open, and Open focuses the new pane', async () => {
    const onOpenPane = vi.fn();
    await mount('pane-a', { from: null, to: view() }, onOpenPane);
    expect(row()!.textContent).toContain('Continued on Fake B · Work');
    expect(row()!.textContent).toContain('17:02');
    expect(row()!.getAttribute('data-keep-going-side')).toBe('old');
    expect(button('Show brief')).toBeUndefined();
    await act(async () => button('Open')!.click());
    expect(onOpenPane).toHaveBeenCalledWith('pane-b');
  });

  it('hides Open when no pane-focus callback is given', async () => {
    await mount('pane-a', { from: null, to: view() });
    expect(row()).toBeTruthy();
    expect(button('Open')).toBeUndefined();
  });
});

describe('KeepGoingBanner: new pane (continued from)', () => {
  it('shows the turns and Show brief, and Show brief loads the stored brief', async () => {
    briefMock.mockResolvedValue('Continue: fix the total\nNext: look at the changes.');
    await mount('pane-b', { from: view(), to: null });
    expect(row()!.getAttribute('data-keep-going-side')).toBe('new');
    expect(row()!.textContent).toContain('14 turns');
    expect(button('Open')).toBeUndefined();
    await act(async () => button('Show brief')!.click());
    expect(briefMock).toHaveBeenCalledWith('kg-0123456789ab');
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog!.textContent).toContain('The brief Fake B · Work got');
    expect(host.querySelector('pre')!.textContent).toBe('Continue: fix the total\nNext: look at the changes.');
  });

  it('uses the singular for one turn', async () => {
    await mount('pane-b', { from: view({ turns: 1 }), to: null });
    expect(row()!.textContent).toContain('1 turn');
    expect(row()!.textContent).not.toContain('1 turns');
  });

  it('shows the brief loading state, then a plain error without the code', async () => {
    let fail: (e: unknown) => void = () => {};
    briefMock.mockReturnValue(new Promise((_, reject) => (fail = reject)));
    await mount('pane-b', { from: view(), to: null });
    await act(async () => button('Show brief')!.click());
    expect(host.textContent).toContain('Loading the brief…');
    await act(async () => fail('BRIEF_MISSING: no brief is stored for this hand-over'));
    const alert = host.querySelector('[role="alert"]');
    expect(alert!.textContent).toBe('No brief is stored for this hand-over');
    expect(host.textContent).not.toContain('BRIEF_MISSING');
  });

  it('Done closes the brief sheet', async () => {
    briefMock.mockResolvedValue('Continue: x');
    await mount('pane-b', { from: view(), to: null });
    await act(async () => button('Show brief')!.click());
    await act(async () => button('Done')!.click());
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  });
});

describe('KeepGoingBanner: states and picking', () => {
  it('failed: says it could not start and to try again from the card, with no buttons', async () => {
    await mount('pane-a', { from: null, to: view({ state: 'failed', stateText: '' }) });
    expect(row()!.getAttribute('data-keep-going')).toBe('failed');
    // With no server text the fallback names the agent.
    expect(row()!.textContent).toContain("Fake B · Work couldn't start");
    expect(row()!.textContent).toContain('Try again from the card');
    expect(host.querySelectorAll('button').length).toBe(0);
  });

  it('uses the server state text as it is', async () => {
    await mount('pane-a', { from: null, to: view({ stateText: '  Moving over to the new agent  ', state: 'starting' }) });
    expect(row()!.textContent).toContain('Moving over to the new agent');
  });

  it('shows the newer record when a pane has both sides', async () => {
    await mount('pane-b', { from: view({ id: 'kg-new', atMs: 2_000 }), to: view({ id: 'kg-old', atMs: 1_000 }) });
    expect(row()!.getAttribute('data-keep-going-side')).toBe('new');
    expect(row()!.textContent).toContain('14 turns');
  });

  it('renders nothing when there is no hand-over, or the lineage cannot be read', async () => {
    await mount('pane-a', { from: null, to: null });
    expect(row()).toBeNull();
    expect(host.innerHTML).toBe('');
    act(() => root.unmount());
    root = createRoot(host);
    await mount('pane-a', new Error('desktop only'));
    expect(row()).toBeNull();
  });
});

describe('KeepGoingBanner: changes', () => {
  it('reloads when keep-going://changed names this pane, and ignores other panes', async () => {
    let fire: (e: { paneIds: string[] }) => void = () => {};
    listenMock.mockImplementation(async (cb: (e: { paneIds: string[] }) => void) => {
      fire = cb;
      return () => {};
    });
    await mount('pane-a', { from: null, to: view() });
    expect(lineageMock).toHaveBeenCalledTimes(1);
    await act(async () => fire({ paneIds: ['some-other-pane'] }));
    expect(lineageMock).toHaveBeenCalledTimes(1);
    lineageMock.mockResolvedValue({ from: null, to: view({ stateText: 'Continued on Fake B · Work · back' }) });
    await act(async () => fire({ paneIds: ['pane-x', 'pane-a'] }));
    expect(lineageMock).toHaveBeenCalledTimes(2);
    expect(row()!.textContent).toContain('back');
  });

  it('stops listening when it unmounts', async () => {
    const stop = vi.fn();
    listenMock.mockResolvedValue(stop);
    await mount('pane-a', { from: null, to: view() });
    act(() => root.unmount());
    root = createRoot(host);
    await act(async () => {});
    expect(stop).toHaveBeenCalledTimes(1);
  });
});

describe('pickHandover', () => {
  it('returns the only side, or the newer of two, or null', () => {
    const a = view({ id: 'a', atMs: 1 });
    const b = view({ id: 'b', atMs: 2 });
    expect(pickHandover({ from: null, to: null })).toBeNull();
    expect(pickHandover({ from: a, to: null })?.id).toBe('a');
    expect(pickHandover({ from: null, to: b })?.id).toBe('b');
    expect(pickHandover({ from: b, to: a })?.id).toBe('b');
    expect(pickHandover({ from: a, to: b })?.id).toBe('b');
  });
});

describe('plainKeepGoingError', () => {
  it('drops the code and keeps the message in plain words', () => {
    expect(plainKeepGoingError('BRIEF_MISSING: no brief is stored for this hand-over')).toBe('No brief is stored for this hand-over');
    expect(plainKeepGoingError(new Error('BAD_TARGET: that agent is not installed'))).toBe('That agent is not installed');
  });

  it('keeps a message that has no code, and falls back when there is no message', () => {
    expect(plainKeepGoingError('The brief is not ready yet.')).toBe('The brief is not ready yet.');
    expect(plainKeepGoingError('CODE_ONLY:')).toBe('Could not load the brief. Try again.');
    expect(plainKeepGoingError(undefined)).toBe('Could not load the brief. Try again.');
    expect(plainKeepGoingError('NOPE', 'Fallback text.')).toBe('NOPE');
    expect(plainKeepGoingError('', 'Fallback text.')).toBe('Fallback text.');
  });
});
