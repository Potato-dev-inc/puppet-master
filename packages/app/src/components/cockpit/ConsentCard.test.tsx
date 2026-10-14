// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { deriveNeedsYou } from '../../lib/attention';
import type { Question } from '../../lib/questions';
import { ConsentCard, consentTitle } from './ConsentCard';

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

const takeOver: Question = {
  id: 'c1',
  agent: 'Claude Code',
  question: 'Allow Claude Code to control Codex?',
  context: 'It would type into the pane and answer its questions. It did not start this pane.',
  choices: [
    { id: 'allow_once', label: 'Allow once' },
    { id: 'always', label: 'Always for this pane' },
    { id: 'deny', label: 'Deny' },
  ],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: 0,
  expires_at_ms: 600_000,
  state: 'pending',
  consent: { action: 'take_over', requester: 'Claude Code', target: 'Codex', pane_id: 'p-codex' },
};

function render(q: Question, paneName?: string) {
  const onAnswer = vi.fn();
  act(() => {
    root.render(
      <ul>
        <ConsentCard q={q} paneName={paneName} now={90_000} onAnswer={onAnswer} />
      </ul>,
    );
  });
  return onAnswer;
}

const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text);

describe('ConsentCard', () => {
  it("asks in the pane's own name with Deny as the primary button", () => {
    render(takeOver, 'Codex 1');
    expect(host.querySelector('.ck-row-title')?.textContent).toBe('Allow Claude Code to control Codex 1?');
    expect(button('Deny')?.className).toContain('ck-btn--primary');
    expect(button('Allow once')?.className).toContain('ck-btn--secondary');
    expect(button('Always for this pane')?.className).toContain('ck-btn--plain');
    expect(host.textContent).toContain('It did not start this pane.');
  });

  it('sends the choice id for each button', () => {
    const onAnswer = render(takeOver);
    act(() => button('Allow once')!.click());
    act(() => button('Always for this pane')!.click());
    act(() => button('Deny')!.click());
    expect(onAnswer.mock.calls.map((c) => c[0])).toEqual([{ choice_id: 'allow_once' }, { choice_id: 'always' }, { choice_id: 'deny' }]);
  });

  it('offers no "always" for answering a risky prompt, and no free text', () => {
    const answer: Question = {
      ...takeOver,
      question: 'x',
      choices: [takeOver.choices[0], takeOver.choices[2]],
      consent: { action: 'answer_prompt', requester: 'Another agent', target: 'Claude Code', risk: 'high' },
    };
    render(answer);
    expect(host.querySelector('.ck-row-title')?.textContent).toBe('Allow another agent to answer a question in Claude Code?');
    expect(button('Always for this pane')).toBeUndefined();
    expect(host.querySelector('input')).toBeNull();
  });

  it('shows no raw ids and falls back to the target name when the pane is closed', () => {
    expect(consentTitle(takeOver)).toBe('Allow Claude Code to control Codex?');
    render(takeOver);
    expect(host.textContent).not.toContain('p-codex');
  });
});

describe('a consent card in the Needs-you list', () => {
  it('counts as one item of its own kind, high risk, not tied to a pane', () => {
    const items = deriveNeedsYou({ panes: [{ id: 'p-codex', status: 'idle' }], questions: [takeOver, { id: 'q2', created_at_ms: 3 }] });
    expect(items.map((i) => [i.id, i.kind])).toEqual([
      ['consent:c1', 'consent'],
      ['question:q2', 'question'],
    ]);
    expect(items[0].risk).toBe('high');
    expect(items[0].paneId).toBeUndefined();
  });
});
