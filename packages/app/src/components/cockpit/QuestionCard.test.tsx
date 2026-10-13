// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { askerName, isLive, timeLeftText, waitCaption, type Question } from '../../lib/questions';
import { QuestionCard } from './QuestionCard';

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

const base: Question = {
  id: 'q1',
  agent: 'Planner',
  question: 'Which database?',
  choices: [
    { id: 'pg', label: 'Postgres' },
    { id: 'sq', label: 'SQLite' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: 0,
  expires_at_ms: 600_000,
  state: 'pending',
};

function render(q: Question, handlers: { onAnswer?: () => void; onDismiss?: () => void } = {}) {
  const onAnswer = vi.fn(handlers.onAnswer);
  const onDismiss = vi.fn(handlers.onDismiss);
  act(() => {
    root.render(
      <ul>
        <QuestionCard q={q} now={90_000} onAnswer={onAnswer} onDismiss={onDismiss} />
      </ul>,
    );
  });
  return { onAnswer, onDismiss };
}

const buttons = () => Array.from(host.querySelectorAll('button'));
const byText = (text: string) => buttons().find((b) => b.textContent === text)!;

function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('QuestionCard', () => {
  it('shows who asks, the question and its choices', () => {
    render(base);
    expect(host.textContent).toContain('Question from Planner');
    expect(host.textContent).toContain('Which database?');
    expect(host.textContent).toContain('asked 1m ago');
    expect(buttons().map((b) => b.textContent)).toEqual(['Postgres', 'SQLite', 'Dismiss']);
    // No text field when free text is off.
    expect(host.querySelector('input')).toBeNull();
  });

  it('answers with the chosen choice id', () => {
    const { onAnswer } = render(base);
    act(() => byText('SQLite').click());
    expect(onAnswer).toHaveBeenCalledExactlyOnceWith({ choice_id: 'sq' });
  });

  it('sends typed free text, trimmed, and clears the field', () => {
    const { onAnswer } = render({ ...base, choices: [], allow_free_text: true, question: 'Which branch?' });
    const input = host.querySelector('input') as HTMLInputElement;
    const send = byText('Send') as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    type(input, '  release/1.2  ');
    expect(send.disabled).toBe(false);
    act(() => send.click());
    expect(onAnswer).toHaveBeenCalledExactlyOnceWith({ text: 'release/1.2' });
    expect(input.value).toBe('');
  });

  it('offers both choices and free text when allowed', () => {
    render({ ...base, allow_free_text: true });
    expect(host.querySelector('input')?.getAttribute('placeholder')).toBe('Or type an answer');
    expect(byText('Postgres')).toBeTruthy();
  });

  it('dismisses', () => {
    const { onDismiss, onAnswer } = render(base);
    act(() => byText('Dismiss').click());
    expect(onDismiss).toHaveBeenCalledOnce();
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it('falls back to the pane name, then a generic asker', () => {
    expect(askerName({ ...base, agent: undefined }, 'Codex 2')).toBe('Codex 2');
    expect(askerName({ ...base, agent: undefined })).toBe('an agent');
  });

  it('treats answered, cancelled and overdue questions as not live', () => {
    expect(isLive(base, 1000)).toBe(true);
    expect(isLive({ ...base, state: 'answered' }, 1000)).toBe(false);
    expect(isLive(base, 600_000)).toBe(false);
  });

  it('gives every choice equal weight: none is the primary button', () => {
    render(base);
    const classes = ['Postgres', 'SQLite'].map((t) => byText(t).className);
    expect(classes.every((c) => c.includes('ck-btn--secondary'))).toBe(true);
    expect(host.querySelector('.ck-btn--primary')).toBeNull();
  });

  it('says how long the agent keeps waiting', () => {
    render(base); // now = 90 s, expires at 600 s: 8.5 min left, rounded up
    expect(host.querySelector('[data-testid="question-wait"]')?.textContent).toBe('Planner stops waiting in 9 min');
  });

  it('words the countdown for the edges', () => {
    expect(timeLeftText(30_000)).toBe('under a minute');
    expect(timeLeftText(8 * 60_000)).toBe('8 min');
    expect(timeLeftText(3 * 3_600_000)).toBe('3 h');
    expect(waitCaption({ ...base, agent: undefined }, 0, 'Codex 2')).toBe('Codex 2 stops waiting in 10 min');
    expect(waitCaption({ ...base, agent: undefined }, 0)).toBe('The agent stops waiting in 10 min');
    expect(waitCaption(base, 600_000)).toBe('Planner has stopped waiting');
  });
});
