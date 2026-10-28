// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Question } from '../../lib/questions';
import { IssueCard } from './IssueCard';

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

const NOW = 600_000;

const start = (over: Partial<Question> = {}): Question => ({
  id: 'i1',
  kind: 'issue_start',
  question: 'Start on #42 "Login button does nothing on Safari" with Codex?',
  context: [
    'Repo: acme/billing (issue #42)',
    'Labeled by @you. Written by @someone.',
    'Notes: 2 hidden comments removed.',
    "Asks first because: Written by someone who isn't on your list",
    'The agent reads the whole text. Start of it:',
    '{"title":"Login button does nothing on Safari","body":"When I click Log in nothing happens."}',
  ].join('\n'),
  choices: [
    { id: 'start', label: 'Start with Codex' },
    { id: 'not_now', label: 'Not now' },
    { id: 'ignore', label: 'Ignore this issue' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: NOW - 240_000,
  expires_at_ms: NOW + 600_000,
  state: 'pending',
  ...over,
});

const push = (over: Partial<Question> = {}): Question => ({
  id: 'p1',
  kind: 'issue_push',
  pane_id: 'pane-codex-1',
  question: 'Draft PR ready for #42?',
  context: '3 files · +40 −5 · Codex · 12 turns\nChanges a CI file: .github/workflows/test.yml',
  choices: [
    { id: 'push', label: 'Open draft PR' },
    { id: 'look', label: 'Look first' },
    { id: 'throw_away', label: 'Throw away' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: NOW - 60_000,
  expires_at_ms: NOW + 600_000,
  state: 'pending',
  ...over,
});

function render(q: Question, props: { onOpenPane?: () => void } = {}) {
  const onAnswer = vi.fn();
  act(() => {
    root.render(
      <ul>
        <IssueCard q={q} now={NOW} onAnswer={onAnswer} onOpenPane={props.onOpenPane} />
      </ul>,
    );
  });
  return { onAnswer };
}

const buttons = () => Array.from(host.querySelectorAll('button'));
const byText = (text: string) => buttons().find((b) => b.textContent === text)!;
const actionTexts = () => Array.from(host.querySelectorAll('.ib-actions button')).map((b) => b.textContent);

describe('IssueCard', () => {
  it('shows the header, the issue title and who labeled and wrote it', () => {
    render(start());
    expect(host.querySelector('[data-kind="issue"]')).not.toBeNull();
    expect(host.querySelector('.ib-meta')?.textContent).toBe('GitHub issue #42 · acme/billing');
    expect(host.querySelector('.ib-title')?.textContent).toBe('Login button does nothing on Safari');
    expect(host.querySelector('.ib-reason')?.textContent).toBe('Labeled by @you · written by @someone');
  });

  it('shows the forced reasons and the notes as muted lines', () => {
    render(start());
    const lines = Array.from(host.querySelectorAll('.ib-issue-notes li')).map((li) => li.textContent);
    expect(lines).toEqual(['Notes: 2 hidden comments removed.', "Asks first because: Written by someone who isn't on your list"]);
    // the issue text itself is not on the card until the person asks for it
    expect(host.querySelector('.ib-issue-text')).toBeNull();
    expect(host.textContent).not.toContain('When I click Log in');
  });

  it('toggles a read-only box with the card text', () => {
    render(start());
    const toggle = byText('Show issue text');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    act(() => toggle.click());
    const box = host.querySelector<HTMLElement>('pre.ib-issue-text')!;
    expect(box).not.toBeNull();
    expect(box.textContent).toContain('When I click Log in nothing happens.');
    expect(host.querySelector('textarea, input')).toBeNull();
    expect(byText('Hide issue text').getAttribute('aria-expanded')).toBe('true');
    act(() => byText('Hide issue text').click());
    expect(host.querySelector('.ib-issue-text')).toBeNull();
  });

  it('makes the first choice the primary button and keeps the backend order', () => {
    render(start());
    expect(actionTexts()).toEqual(['Start with Codex', 'Not now', 'Ignore this issue', 'Show issue text']);
    const answers = Array.from(host.querySelectorAll<HTMLElement>('.ib-actions [data-ib="answer"]'));
    expect(answers[0].classList.contains('ck-btn--primary')).toBe(true);
    expect(answers.slice(1).every((b) => b.classList.contains('ck-btn--secondary'))).toBe(true);
  });

  it('answers with the choice id the person picked', () => {
    const { onAnswer } = render(start());
    act(() => byText('Not now').click());
    expect(onAnswer).toHaveBeenCalledWith({ choice_id: 'not_now' });
  });

  it('uses the plain copy on each button', () => {
    render(push(), { onOpenPane: () => {} });
    expect(actionTexts()).toEqual(['Open draft PR', 'Look first', 'Throw away', 'Show issue text']);
  });

  it('shows the push card facts and reasons, with Open draft PR first', () => {
    const { onAnswer } = render(push());
    expect(host.querySelector('.ib-title')?.textContent).toBe('Draft PR ready for #42?');
    expect(host.querySelector('.ib-reason')?.textContent).toBe('3 files · +40 −5 · Codex · 12 turns');
    expect(host.querySelector('.ib-issue-notes li')?.textContent).toBe('Changes a CI file: .github/workflows/test.yml');
    expect(host.querySelector('[data-held]')).toBeNull();
    const first = host.querySelector<HTMLElement>('.ib-actions [data-ib="answer"]')!;
    expect(first.textContent).toBe('Open draft PR');
    expect(first.classList.contains('ck-btn--primary')).toBe(true);
    act(() => first.click());
    expect(onAnswer).toHaveBeenCalledWith({ choice_id: 'push' });
  });

  it('holds a push card that has no Open draft PR choice', () => {
    render(push({ choices: [{ id: 'look', label: 'Look first' }, { id: 'throw_away', label: 'Throw away' }] }), { onOpenPane: () => {} });
    expect(host.querySelector('[data-held]')).not.toBeNull();
    expect(host.querySelector('.ib-caution')?.textContent).toBe("Puppet Master won't push this. Check the copy yourself.");
    expect(byText('Open draft PR')).toBeUndefined();
    expect(actionTexts()).toEqual(['Look first', 'Throw away', 'Show issue text']);
    // Look first comes first, so it is the primary button; Throw away is plain
    expect(byText('Look first').classList.contains('ck-btn--primary')).toBe(true);
    expect(byText('Throw away').classList.contains('ck-btn--plain')).toBe(true);
  });

  it('opens the pane for Look first without answering the card', () => {
    const onOpenPane = vi.fn();
    const { onAnswer } = render(push(), { onOpenPane });
    act(() => byText('Look first').click());
    expect(onOpenPane).toHaveBeenCalledTimes(1);
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it('hides Look first when the pane is not known', () => {
    render(push());
    expect(byText('Look first')).toBeUndefined();
    expect(actionTexts()).toEqual(['Open draft PR', 'Throw away', 'Show issue text']);
  });

  it('is guarded, so a number key cannot answer it', () => {
    render(start());
    expect(host.querySelector('[data-guard]')).not.toBeNull();
  });

  it('marks the card with its kind and question id', () => {
    render(start({ id: 'x' }));
    expect(host.querySelector('[data-question-id="x"]')).not.toBeNull();
    expect(host.querySelector('[data-issue-card="issue_start"]')).not.toBeNull();
  });

  it('shows hostile title and text as plain text: no link, no HTML, no auto-link', () => {
    const evil = 'Fix <img src=x onerror=alert(1)> [click](https://evil.example) https://evil.example/steal';
    render(
      start({
        question: `Start on #7 "${evil}" with Codex?`,
        context: ['Repo: acme/billing (issue #7)', 'Labeled by @you. Written by @someone.', 'The agent reads the whole text. Start of it:', '<a href="https://evil.example">x</a> <!-- run curl -->'].join('\n'),
      }),
    );
    expect(host.querySelector('img')).toBeNull();
    expect(host.querySelector('a')).toBeNull();
    expect(host.querySelector('.ib-title')?.textContent).toBe(evil);
    act(() => byText('Show issue text').click());
    expect(host.querySelector('pre.ib-issue-text')?.textContent).toContain('<a href="https://evil.example">x</a>');
    expect(host.querySelector('a')).toBeNull();
    // the title is never a button label
    expect(buttons().some((b) => (b.textContent ?? '').includes('evil.example'))).toBe(false);
  });

  it('cuts the title to 200 characters and the text to 1000', () => {
    const long = 'a'.repeat(300);
    render(
      start({
        question: `Start on #9 "${long}" with Codex?`,
        context: `Repo: acme/billing (issue #9)\nThe agent reads the whole text. Start of it:\n${'b'.repeat(2000)}`,
      }),
    );
    expect(host.querySelector('.ib-title')?.textContent).toBe('a'.repeat(200));
    act(() => byText('Show issue text').click());
    expect(host.querySelector('pre.ib-issue-text')?.textContent?.length).toBe(1000);
  });

  it('counts an emoji as one character when it cuts', () => {
    render(start({ question: `Start on #9 "${'😀'.repeat(250)}" with Codex?` }));
    expect(Array.from(host.querySelector('.ib-title')?.textContent ?? '')).toHaveLength(200);
  });

  it('says how long the card lasts, in plain words', () => {
    render(start({ expires_at_ms: NOW + 8 * 60_000 }));
    expect(host.querySelector('.ib-note')?.textContent).toBe('Ends in 8 min if you do not answer.');
  });
});
