// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import {
  LoopGuardContext,
  useLoopSignals,
  type LoopGuardApi,
  type LoopSignal,
  type StopReport,
} from '../../lib/loop-guard';
import { Inbox } from './Inbox';
import { PanicSheet } from './PanicSheet';
import { Palette } from './Palette';
import { OPEN_NEW_MENU, PaneStuckCard, StuckCard } from './StuckCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (Element.prototype as { scrollIntoView?: () => void }).scrollIntoView = () => {};
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const signal: LoopSignal = {
  pane_id: 'p1',
  kind: 'repeated_error',
  evidence_lines: ['error[E0432]: unresolved import `crate::store`', 'cargo build failed'],
  turns: 4,
  since: 1,
  summary: 'the same error 4 times',
};
const button = (text: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;

const cockpit = (over: Record<string, unknown> = {}) =>
  ({
    registry: {
      paneList: [{ status: 'idle', info: { id: 'p1', agent_type: 'claude', cwd: 'C:\\w\\app' } }],
      panes: new Map([['p1', {}]]),
      initialReady: true,
    },
    prompts: {},
    asked: { questions: [] },
    statusSince: new Map(),
    names: new Map([['p1', 'Claude Code']]),
    launchEntries: [],
    readyAgents: new Set(),
    now: 0,
    selectedId: 'p1',
    answer: vi.fn(),
    select: vi.fn(),
    interrupt: vi.fn(),
    ...over,
  }) as unknown as CockpitApi;

describe('stuck card', () => {
  it('says what is wrong in one plain sentence, shows the evidence and the four actions', () => {
    act(() =>
      root.render(
        <StuckCard name="Claude Code" signal={signal} onHint={() => {}} onInterrupt={() => {}} onAsk={() => {}} onDismiss={() => {}} />,
      ),
    );
    expect(host.querySelector('.ib-title')!.textContent).toBe('Claude Code seems stuck: the same error 4 times.');
    expect(Array.from(host.querySelectorAll('.ib-evidence li')).map((l) => l.textContent)).toEqual(signal.evidence_lines);
    expect(Array.from(host.querySelectorAll('button')).map((b) => b.textContent)).toEqual([
      'Give it a hint',
      'Interrupt',
      'Ask another agent',
      'Dismiss',
    ]);
  });

  it('wires the actions: hint focuses the terminal, interrupt, ask opens the plus menu, dismiss', () => {
    const c = cockpit({ selectedId: null });
    const dismiss = vi.fn();
    const opened = vi.fn();
    window.addEventListener(OPEN_NEW_MENU, opened);
    act(() =>
      root.render(
        <LoopGuardContext.Provider value={{ signals: { p1: signal }, dismiss }}>
          <PaneStuckCard c={c} paneId="p1" />
        </LoopGuardContext.Provider>,
      ),
    );
    act(() => button('Give it a hint')!.click());
    expect(c.select).toHaveBeenCalledWith('p1');
    act(() => button('Interrupt')!.click());
    expect(c.interrupt).toHaveBeenCalledWith('p1');
    act(() => button('Ask another agent')!.click());
    expect(opened).toHaveBeenCalledTimes(1);
    act(() => button('Dismiss')!.click());
    expect(dismiss).toHaveBeenCalledWith('p1');
    window.removeEventListener(OPEN_NEW_MENU, opened);
  });

  it('renders nothing for a pane that is not stuck, and shows up in the Inbox when it is', () => {
    const c = cockpit();
    act(() => root.render(<PaneStuckCard c={c} paneId="p1" />));
    expect(host.querySelector('[data-kind="stuck"]')).toBeNull();
    act(() =>
      root.render(
        <LoopGuardContext.Provider value={{ signals: { p1: signal }, dismiss: () => {} }}>
          <Inbox c={c} />
        </LoopGuardContext.Provider>,
      ),
    );
    // in the Inbox it is a card in the one list, spaced by the list (no dock margin)
    expect(host.querySelector('[aria-label="Needs you"] [data-kind="stuck"]')).not.toBeNull();
    expect(host.querySelector('.ib-dock')).toBeNull();
    expect(host.textContent).toContain('Claude Code seems stuck: the same error 4 times.');
  });
});

describe('useLoopSignals', () => {
  function fakeApi() {
    let onSignal: (s: LoopSignal) => void = () => {};
    let onClear: (id: string) => void = () => {};
    const api = {
      signals: async () => [signal],
      dismiss: vi.fn(async () => true),
      onSignal: async (cb: (s: LoopSignal) => void) => ((onSignal = cb), () => {}),
      onClear: async (cb: (id: string) => void) => ((onClear = cb), () => {}),
    } as unknown as LoopGuardApi;
    return { api, fire: (s: LoopSignal) => onSignal(s), clear: (id: string) => onClear(id) };
  }
  function Probe({ api }: { api: LoopGuardApi }) {
    const { signals, dismiss } = useLoopSignals(api);
    return (
      <div>
        <span id="ids">{Object.keys(signals).join(',')}</span>
        <button onClick={() => dismiss('p1')}>dismiss</button>
      </div>
    );
  }
  const ids = () => host.querySelector('#ids')!.textContent;
  const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

  it('loads current warnings, follows events and dismisses', async () => {
    const { api, fire, clear } = fakeApi();
    await act(async () => root.render(<Probe api={api} />));
    await flush();
    expect(ids()).toBe('p1');
    await act(async () => fire({ ...signal, pane_id: 'p2' }));
    expect(ids()).toBe('p1,p2');
    await act(async () => clear('p2'));
    expect(ids()).toBe('p1');
    await act(async () => host.querySelector('button')!.click());
    expect(ids()).toBe('');
    expect(api.dismiss).toHaveBeenCalledWith('p1');
  });
});

describe('panic sheet', () => {
  const names = new Map([['p1', 'Claude Code']]);
  it('lists what was stopped', () => {
    const report: StopReport = {
      stopped: [
        { pane_id: 'p1', agent_type: 'claude', cwd: 'C:\\w\\app' },
        { pane_id: 'p2', agent_type: 'codex', cwd: 'C:\\w\\api' },
      ],
      failed: [['p3', 'pane has exited']],
    };
    const onClose = vi.fn();
    act(() => root.render(<PanicSheet report={report} names={names} onClose={onClose} />));
    expect(host.querySelector('#ck-panic-t')!.textContent).toBe('Stopped 2 agents');
    const rows = Array.from(host.querySelectorAll('.ck-panic-list')[0].querySelectorAll('li')).map((l) => l.textContent);
    expect(rows).toEqual(['Claude Codeapp', 'codexapi']);
    expect(host.textContent).toContain('pane has exited');
    act(() => button('Done')!.click());
    expect(onClose).toHaveBeenCalled();
  });

  it('says so when nothing was running', () => {
    act(() => root.render(<PanicSheet report={{ stopped: [], failed: [] }} names={names} onClose={() => {}} />));
    expect(host.querySelector('#ck-panic-t')!.textContent).toBe('Nothing was running');
  });

  it('is one palette entry, "Stop all agents"', () => {
    const stop = vi.fn();
    act(() => root.render(<Palette c={cockpit()} onClose={() => {}} onStopAll={stop} />));
    const row = Array.from(host.querySelectorAll('.ck-palette-row')).find((r) => r.textContent?.includes('Stop all agents'));
    expect(row).toBeDefined();
    act(() => (row as HTMLElement).click());
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
