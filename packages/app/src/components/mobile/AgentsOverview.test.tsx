// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import type { Question } from '../../lib/questions';
import type { LoopSignalInfo } from '../../lib/bridge-types';
import type { AttentionSnapshot } from '../../lib/attention';
import { AgentsOverview } from './AgentsOverview';
import { resetDigestCardForTests } from './DigestCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const pane = (id: string, status: string, created_at: number, extra = {}) =>
  ({
    id,
    status,
    created_at,
    agent_type: 'claude',
    cwd: 'C:\\work\\proj',
    pid: 1,
    last_output_at: Date.now() - 120000,
    cols: 44,
    rows: 30,
    ...extra,
  }) as unknown as PaneInfo;

const QUESTION = {
  id: 'q-1',
  agent: 'Claude Code',
  pane_id: null,
  question: 'Keep the old route?',
  choices: [{ id: 'keep', label: 'Keep it' }],
  allow_free_text: false,
  context: null,
  urgency: 'normal',
  created_at_ms: Date.now(),
  expires_at_ms: Date.now() + 3_600_000,
  state: 'pending',
} as unknown as Question;

const fakeBridge = {
  readBuffer: async () => 'Working on the thing\nsecond meaningful line here',
  digest: async () => ({ empty: true }),
  listQuestions: async () => [QUESTION],
} as unknown as BridgeClient;

const labels = new Map([
  ['a', 'Fix login'],
  ['c', 'powershell'],
  ['b', 'Billing tests'],
  ['d', 'opencode'],
]);

const panes = [
  pane('a', 'waiting_input', 1),
  pane('c', 'error', 2),
  pane('b', 'running', 3),
  pane('d', 'idle', 4),
];

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  resetDigestCardForTests();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function mount(
  props: Partial<{
    panes: PaneInfo[];
    bridgeReady: boolean;
    onOpen: (id: string) => void;
    onNew: () => void;
    stuck: Map<string, LoopSignalInfo>;
    questionsRevision: number;
    attention: AttentionSnapshot | null;
  }> = {},
) {
  const onOpen = props.onOpen ?? vi.fn();
  const onNew = props.onNew ?? vi.fn();
  await act(async () => {
    root.render(
      <AgentsOverview
        bridge={fakeBridge}
        bridgeReady={props.bridgeReady ?? true}
        panes={props.panes ?? panes}
        labels={labels}
        onOpen={onOpen}
        onNew={onNew}
        stuck={props.stuck}
        questionsRevision={props.questionsRevision}
        attention={props.attention}
      />,
    );
  });
  // Let the preview poll resolve.
  for (let i = 0; i < 3; i++) {
    await act(async () => {});
  }
  return { onOpen, onNew };
}

const sections = () => Array.from(host.querySelectorAll<HTMLElement>('section'));
const section = (name: string) => sections().find((s) => s.getAttribute('aria-label') === name)!;
const row = (title: string) =>
  Array.from(host.querySelectorAll<HTMLButtonElement>('button.ph-row')).find(
    (b) => b.querySelector('.ph-t')?.textContent === title,
  )!;

describe('phone AgentsOverview', () => {
  it('shows the Agents title and the waiting counter', async () => {
    await mount();
    expect(host.querySelector('h1')?.textContent).toBe('Agents');
    expect(host.querySelector('.ph-doto')?.textContent).toBe('02');
  });

  it('shows the PC attention count when the PC sends one, not its own count', async () => {
    // Two panes are waiting or failed here (02), but the PC says five things need you: a question, a
    // stuck pane and a consent card count too. The phone shows the PC's number.
    const attention = { v: 1, rev: 9, counts: { blocking: 5, ready: 1 }, items: [] } as AttentionSnapshot;
    await mount({ attention });
    expect(host.querySelector('.ph-doto')?.textContent).toBe('05');
    expect(host.querySelector('.ph-counter')?.getAttribute('aria-label')).toBe('5 waiting');
  });

  it('counts its own panes when the PC is too old to send attention', async () => {
    await mount({ attention: null });
    expect(host.querySelector('.ph-doto')?.textContent).toBe('02');
  });

  it('groups the panes into Needs you, Working and Idle, in that order', async () => {
    await mount();
    const captions = Array.from(host.querySelectorAll('.ph-sec')).map((h) => h.textContent);
    expect(captions).toEqual(['Needs you', 'Working', 'Idle']);
  });

  it('puts waiting and error panes under Needs you as hot rows', async () => {
    await mount();
    const needs = section('Needs you');
    const titles = Array.from(needs.querySelectorAll('.ph-t')).map((t) => t.textContent);
    expect(titles).toEqual(['Fix login', 'powershell']);
    for (const r of needs.querySelectorAll('button.ph-row')) {
      expect(r.classList.contains('ph-row--hot')).toBe(true);
    }
    expect(section('Working').textContent).toContain('Billing tests');
    expect(section('Idle').textContent).toContain('opencode');
  });

  it('opens the pane when its row is tapped', async () => {
    const { onOpen } = await mount();
    act(() => row('Billing tests').click());
    expect(onOpen).toHaveBeenCalledWith('b');
  });

  it('shows a stalled signal in place of the last output line', async () => {
    const stuck = new Map([['b', { kind: 'stalled', summary: 'no output for 20 minutes' } as unknown as LoopSignalInfo]]);
    await mount({ stuck });
    expect(row('Billing tests').textContent).toContain('Looks stuck: no output for 20 minutes');
  });

  it('shows the last preview line of the screen when nothing is stuck', async () => {
    await mount();
    expect(row('Billing tests').textContent).toContain('second meaningful line here');
  });

  it('renders the empty state with an Open an agent button when there are no panes', async () => {
    const { onNew } = await mount({ panes: [] });
    expect(host.textContent).toContain('No agents running');
    const open = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Open an agent')!;
    expect(open).toBeTruthy();
    act(() => open.click());
    expect(onNew).toHaveBeenCalledTimes(1);
  });

  it('renders a question card once when there are no panes', async () => {
    await mount({ panes: [], questionsRevision: 0 });
    expect(host.querySelectorAll('[data-question-id]')).toHaveLength(1);
    expect(host.querySelectorAll('[aria-label="Questions from agents"]')).toHaveLength(1);
  });

  it('shows the connecting message until the bridge is ready', async () => {
    await mount({ bridgeReady: false });
    expect(host.textContent).toContain('Connecting to your PC');
    expect(host.querySelector('h1')).toBeNull();
  });
});
