// @vitest-environment jsdom
// The Inbox hero (Doto), the Inbox badge in the sidebar, the sidebar "Needs you" group and the phone
// counter all show the length of ONE list. This renders them on one mixed fixture and compares.
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import { blockingItems, deriveNeedsYou } from '../../lib/attention';
import { NeedsYouContext } from '../../hooks/useNeedsYou';
import { LoopGuardContext, type LoopSignal } from '../../lib/loop-guard';
import { waitingCount } from '../../lib/mobile-agent-tabs';
import type { Question } from '../../lib/questions';
import { Inbox } from './Inbox';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

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

const info = (id: string, agent: string, status: string): PaneInfo =>
  ({ id, agent_type: agent, pid: 1, status, created_at: 1, last_output_at: null, cwd: 'C:\\work\\proj', cols: 80, rows: 24, title: null }) as unknown as PaneInfo;

const question: Question = {
  id: 'q1',
  agent: 'Planner',
  question: 'Which database?',
  choices: [{ id: 'pg', label: 'Postgres' }],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: 0,
  expires_at_ms: 1_000_000,
  state: 'pending',
};

const stuck: LoopSignal = { pane_id: 'run', kind: 'repeated_error', evidence_lines: ['boom'], turns: 3, since: 0, summary: 'the same error 3 times' };

// Two approvals, one errored PowerShell, one running pane that looks stuck, one question: five things.
function fixture() {
  const list = [
    { info: info('ap1', 'claude', 'waiting_input'), status: 'waiting_input' },
    { info: info('ap2', 'codex', 'waiting_input'), status: 'waiting_input' },
    { info: info('ps', 'powershell', 'error'), status: 'error' },
    { info: info('run', 'claude', 'running'), status: 'running' },
    { info: info('idle', 'codex', 'idle'), status: 'idle' },
  ];
  const select = vi.fn();
  const api = {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([
      ['ap1', 'Claude Code'],
      ['ap2', 'Codex'],
      ['ps', 'PowerShell'],
      ['run', 'Claude Code 2'],
      ['idle', 'Codex 2'],
    ]),
    asked: { questions: [question], answerQuestion: vi.fn(), dismissQuestion: vi.fn() },
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: null,
    select,
    prompts: {},
    statusSince: new Map(list.map((p) => [p.info.id, 0])),
    now: 120_000,
    spawn: null,
    answer: vi.fn(),
    interrupt: vi.fn(),
  } as unknown as CockpitApi;
  return { api, select, list };
}

function renderWithStuck(node: ReactNode) {
  act(() => {
    root.render(<LoopGuardContext.Provider value={{ signals: { run: stuck }, dismiss: () => {} }}>{node}</LoopGuardContext.Provider>);
  });
}

describe('one Needs-you count', () => {
  it('shows the same number on the Doto counter, the Inbox badge and the sidebar group', () => {
    const { api } = fixture();
    renderWithStuck(
      <>
        <Sidebar c={api} onOpenPalette={() => {}} />
        <Inbox c={api} />
      </>,
    );
    const doto = host.querySelector('.ck-doto')!;
    const badge = host.querySelector('.ck-row--flat .ck-count')!;
    const group = host.querySelector('.ck-group .ck-caption-count')!;
    expect(doto.textContent).toBe('05');
    expect(badge.textContent).toBe('5');
    expect(group.textContent).toBe('5');
    expect(doto.getAttribute('aria-label')).toBe('5 things need you');
  });

  it('does not count an Ask-bar answer: it waits under Ready for you', () => {
    const { api, list } = fixture();
    const items = deriveNeedsYou({
      panes: list.map((p) => ({ id: p.info.id, status: p.status })),
      questions: [question],
      stuckPaneIds: ['run'],
      notices: [{ id: 'ask-1', at_ms: 5 }],
    });
    expect(items).toHaveLength(6);
    expect(blockingItems(items)).toHaveLength(5);
    act(() => {
      root.render(
        <LoopGuardContext.Provider value={{ signals: { run: stuck }, dismiss: () => {} }}>
          <NeedsYouContext.Provider value={items}>
            <Sidebar c={api} onOpenPalette={() => {}} />
            <Inbox c={api} />
          </NeedsYouContext.Provider>
        </LoopGuardContext.Provider>,
      );
    });
    expect(host.querySelector('.ck-doto')!.textContent).toBe('05');
    expect(host.querySelector('.ck-row--flat .ck-count')!.textContent).toBe('5');
    expect(host.querySelector('.ck-group .ck-caption-count')!.textContent).toBe('5');
  });

  it('puts every pane that needs you under the sidebar Needs you group, including the errored and the stuck one', () => {
    const { api } = fixture();
    renderWithStuck(<Sidebar c={api} onOpenPalette={() => {}} />);
    const needs = Array.from(host.querySelectorAll('.ck-group')).find((g) => g.querySelector('.ck-caption')?.textContent?.startsWith('Needs you'))!;
    const titles = Array.from(needs.querySelectorAll('.ck-row-title')).map((t) => t.textContent);
    expect(titles.sort()).toEqual(['Claude Code', 'Claude Code 2', 'Codex', 'PowerShell']);
  });

  it('the phone counter is the same derivation', () => {
    const { list } = fixture();
    const phonePanes = list.map((p) => p.info);
    const asked = [question];
    const phone = waitingCount(phonePanes, { questions: asked, stuckPaneIds: ['run'] });
    expect(phone).toBe('05');
    // An Ask-bar answer is not a number on the phone either.
    expect(waitingCount(phonePanes, { questions: asked, stuckPaneIds: ['run'], notices: [{ id: 'n' }] })).toBe('05');
    const desktop = deriveNeedsYou({
      panes: list.map((p) => ({ id: p.info.id, status: p.status })),
      questions: asked,
      stuckPaneIds: ['run'],
    });
    expect(String(desktop.length).padStart(2, '0')).toBe(phone);
  });
});

describe('Inbox error card', () => {
  it('shows an errored pane as a card with Open and Ask another agent', () => {
    const { api, select } = fixture();
    renderWithStuck(<Inbox c={api} />);
    const card = host.querySelector('[data-error-pane="ps"]') as HTMLElement;
    expect(card).toBeTruthy();
    expect(card.textContent).toContain('PowerShell hit an error');
    const labels = Array.from(card.querySelectorAll('button')).map((b) => b.textContent);
    expect(labels).toEqual(['Open', 'Ask another agent']);
    act(() => (card.querySelector('button') as HTMLElement).click());
    expect(select).toHaveBeenCalledWith('ps');
  });

  it('"Ask another agent" opens the new-agent menu', () => {
    const { api } = fixture();
    renderWithStuck(<Inbox c={api} />);
    const seen = vi.fn();
    window.addEventListener('pm:open-new-menu', seen);
    const ask = Array.from(host.querySelectorAll('[data-error-pane="ps"] button')).find((b) => b.textContent === 'Ask another agent') as HTMLElement;
    act(() => ask.click());
    window.removeEventListener('pm:open-new-menu', seen);
    expect(seen).toHaveBeenCalledOnce();
  });

  it('says nothing needs you when all is calm', () => {
    const { api } = fixture();
    const only = api.registry.paneList.filter((p) => p.status === 'idle');
    const calm = { ...api, asked: { ...api.asked, questions: [] }, registry: { ...api.registry, paneList: only, panes: new Map(only.map((p) => [p.info.id, p])) } } as unknown as CockpitApi;
    act(() => root.render(<Inbox c={calm} />));
    expect(host.querySelector('.ck-doto')?.textContent).toBe('00');
    expect(host.querySelector('h1')?.textContent).toBe('Nothing needs you');
  });
});
