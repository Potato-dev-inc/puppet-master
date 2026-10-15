// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { deriveNeedsYou } from '../../lib/attention';
import { isLimit, type Question } from '../../lib/questions';
import { LimitCard } from './LimitCard';

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

const usage: Question = {
  id: 'l1',
  agent: 'Claude Code',
  pane_id: 'p-claude',
  question: 'Claude Code ran out of usage. It can continue at 14:05.',
  context: 'Handing the work to another agent is coming in a later update.',
  choices: [
    { id: 'remind', label: 'Remind me at 14:05' },
    { id: 'stop', label: 'Stop here' },
  ],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: 0,
  expires_at_ms: 43_200_000,
  state: 'pending',
  limit: { kind: 'usage_limit', agent_label: 'Claude Code', window: 'session limit', reset_at_ms: 1, has_run: true },
};

function render(q: Question, extra: { paneName?: string; onOpenPane?: () => void } = {}) {
  const onAnswer = vi.fn();
  act(() => {
    root.render(
      <ul>
        <LimitCard q={q} now={120_000} onAnswer={onAnswer} {...extra} />
      </ul>,
    );
  });
  return onAnswer;
}

const buttons = () => [...host.querySelectorAll('button')].map((b) => b.textContent);
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text);

describe('LimitCard', () => {
  it('says what happened, when it can continue, and offers exactly two choices', () => {
    render(usage);
    expect(host.querySelector('.ib-title')?.textContent).toBe('Claude Code ran out of usage. It can continue at 14:05.');
    // the one age format, no "ran out" prefix
    expect(host.querySelector('.ib-age')?.textContent).toBe('2m');
    expect(buttons()).toEqual(['Remind me at 14:05', 'Stop here']);
    expect(button('Remind me at 14:05')!.className).toContain('ck-btn--primary');
  });

  it('says the hand-off is coming later and has no hand-off button', () => {
    render(usage, { onOpenPane: () => {} });
    expect(host.textContent).toContain('coming in a later update');
    for (const label of buttons()) {
      expect(label).not.toMatch(/hand|continue with|resume|switch|cancel/i);
    }
  });

  it('never suggests another login of the same vendor', () => {
    render(usage, { paneName: 'Claude Code 1', onOpenPane: () => {} });
    expect(host.textContent ?? '').not.toMatch(/login|account|personal|use your|switch to/i);
  });

  it('answers with the choice ids the backend reads', () => {
    const onAnswer = render(usage);
    act(() => button('Remind me at 14:05')!.click());
    act(() => button('Stop here')!.click());
    expect(onAnswer.mock.calls).toEqual([[{ choice_id: 'remind' }], [{ choice_id: 'stop' }]]);
  });

  it('names the pane and can open its terminal', () => {
    const open = vi.fn();
    render(usage, { paneName: 'Claude Code 1', onOpenPane: open });
    expect(host.querySelector('.ib-meta')?.textContent).toBe('Claude Code 1');
    act(() => button('Open terminal')!.click());
    expect(open).toHaveBeenCalledOnce();
  });

  it('with its own wait on, the backend says so and the card shows it', () => {
    render({ ...usage, context: 'Claude Code is set to continue on its own. Handing the work to another agent is coming in a later update.' });
    expect(host.querySelector('.ib-reason')?.textContent).toContain('set to continue on its own');
  });

  it('a limit with no printed reset says so and offers a reminder in an hour', () => {
    render({
      ...usage,
      question: "Claude Code ran out of usage. It didn't say when it can continue.",
      choices: [
        { id: 'remind', label: 'Remind me in 1 hour' },
        { id: 'stop', label: 'Stop here' },
      ],
    });
    expect(buttons()).toEqual(['Remind me in 1 hour', 'Stop here']);
  });

  it('a pane without a run has nothing to stop: Dismiss', () => {
    render({ ...usage, choices: [usage.choices[0], { id: 'stop', label: 'Dismiss' }], limit: { ...usage.limit!, has_run: false } });
    expect(buttons()).toEqual(['Remind me at 14:05', 'Dismiss']);
  });

  it('the reminder card has one button and says the limit should have reset', () => {
    const onAnswer = render({
      ...usage,
      id: 'r1',
      question: "Claude Code's usage limit should have reset by now.",
      choices: [{ id: 'ok', label: 'Got it' }],
      limit: { kind: 'reset_reminder', agent_label: 'Claude Code' },
    });
    expect(buttons()).toEqual(['Got it']);
    expect(host.querySelector('.ib-age')?.textContent).toBe('2m');
    act(() => button('Got it')!.click());
    expect(onAnswer).toHaveBeenCalledWith({ choice_id: 'ok' });
  });
});

describe('a limit card in the Needs you number', () => {
  it('counts once, as its own kind, on the pane it belongs to', () => {
    expect(isLimit(usage)).toBe(true);
    const items = deriveNeedsYou({ panes: [{ id: 'p-claude', status: 'waiting_input' }], questions: [usage] });
    expect(items.map((i) => i.id)).toEqual(['approval:p-claude', 'limit:l1']);
    expect(items[1]).toMatchObject({ kind: 'limit', paneId: 'p-claude' });
  });
});
