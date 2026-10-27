// @vitest-environment jsdom
// A helper's question in its parent's Chat: the Inbox's own card, answered through the same route.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { PanePrompt } from '../../lib/cockpit-api';
import { HelperQuestions, helperQuestionIds } from './HelperQuestions';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const mk = (id: string, status: string, parent?: string, agent = 'claude') => ({
  status,
  info: { id, agent_type: agent, status, pid: 1, created_at: 1, last_output_at: null, cwd: 'C:\work\shop', cols: 80, rows: 24, parent_pane_id: parent },
});
const colour: PanePrompt = {
  promptId: 'p-1',
  kind: 'question',
  text: 'Pick a colour',
  summary: 'Pick a colour',
  choices: [
    { id: '1', label: 'Red' },
    { id: '2', label: 'Blue' },
  ],
};

function cockpit(list: ReturnType<typeof mk>[], prompts: Record<string, PanePrompt>, answer = vi.fn()): CockpitApi {
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map(list.map((p) => [p.info.id, p.info.agent_type === 'codex' ? 'Codex' : 'Claude Code'])),
    prompts,
    statusSince: new Map(),
    now: 12_000,
    select: vi.fn(),
    notify: vi.fn(),
    answer,
  } as unknown as CockpitApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const list = [mk('P', 'running'), mk('H', 'waiting_input', 'P', 'codex'), mk('G', 'running', 'H')];

describe('helperQuestionIds', () => {
  it('lists a helper that waits on a card with choices', () => {
    expect(helperQuestionIds(cockpit(list, { H: colour }), 'P')).toEqual(['H']);
  });
  it('lists nothing once the helper is no longer waiting, or the card has no choices', () => {
    expect(helperQuestionIds(cockpit([mk('P', 'running'), mk('H', 'running', 'P')], { H: colour }), 'P')).toEqual([]);
    expect(helperQuestionIds(cockpit(list, { H: { ...colour, choices: [] } }), 'P')).toEqual([]);
    expect(helperQuestionIds(cockpit(list, {}), 'P')).toEqual([]);
  });
  it('shows a grandchild question to its parent and to the top pane', () => {
    const deep = [mk('P', 'running'), mk('H', 'running', 'P'), mk('G', 'waiting_input', 'H')];
    const c = cockpit(deep, { G: colour });
    expect(helperQuestionIds(c, 'H')).toEqual(['G']);
    expect(helperQuestionIds(c, 'P')).toEqual(['G']);
  });
});

describe('HelperQuestions', () => {
  it('draws the helper card with its choices, answers through c.answer with the prompt id, and opens the helper', async () => {
    const answer = vi.fn();
    const c = cockpit(list, { H: colour }, answer);
    await act(async () => root.render(<HelperQuestions c={c} ids={['H']} />));
    expect(host.querySelector('.ib-title')?.textContent).toBe('Codex helper asks: Pick a colour');
    const buttons = [...host.querySelectorAll<HTMLButtonElement>('[data-ib="answer"]')];
    expect(buttons.map((b) => b.textContent)).toEqual(['Red', 'Blue']);
    await act(async () => buttons[0].click());
    expect(answer).toHaveBeenCalledTimes(1);
    expect(answer.mock.calls[0].slice(0, 4)).toEqual(['H', 'p-1', '1', 'Red']);
    // The click is the permit: it travels as the gesture argument.
    expect(answer.mock.calls[0][4]).toMatchObject({ kind: 'click' });
    const open = host.querySelector<HTMLButtonElement>('[data-ib="open"]')!;
    expect(open.textContent).toBe('Open helper');
    await act(async () => open.click());
    expect(c.select).toHaveBeenCalledWith('H');
  });

  it('renders nothing without questions', async () => {
    await act(async () => root.render(<HelperQuestions c={cockpit(list, {})} ids={[]} />));
    expect(host.innerHTML).toBe('');
  });
});
