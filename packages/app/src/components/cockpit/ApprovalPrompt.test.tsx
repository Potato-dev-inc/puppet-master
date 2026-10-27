// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PanePrompt } from '../../lib/cockpit-api';
import { LEVEL_WORD, choiceWords, isBroadChoice, layoutChoices } from '../../lib/approval-risk';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { ApprovalPrompt } from './ApprovalPrompt';

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

const choices = [
  { id: 'allow_once', label: 'Allow once' },
  { id: 'allow_all', label: 'Always allow', broad: true },
  { id: 'deny', label: 'Deny' },
];

const prompt = (level: 'low' | 'medium' | 'high', headline: string): PanePrompt => ({
  promptId: `p-${level}`,
  kind: 'command_approval',
  text: 'raw',
  summary: 'Run command: rm -rf ../old',
  choices,
  risk: { level, headline, reasons: ['It deletes ../old and everything in it.', 'That is outside this project.'] },
});

function render(p: PanePrompt, onAnswer = vi.fn()) {
  act(() => root.render(<ApprovalPrompt prompt={p} onAnswer={onAnswer} variant="card" />));
  return onAnswer;
}
const btn = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text);

describe('layoutChoices', () => {
  it('keeps every choice in view for low and medium risk, first one primary', () => {
    for (const level of ['low', 'medium'] as const) {
      const l = layoutChoices(prompt(level, 'x'));
      expect(l.visible.map((c) => c.id)).toEqual(['allow_once', 'allow_all', 'deny']);
      expect(l.more).toEqual([]);
      expect(l.primaryId).toBe('allow_once');
    }
  });

  it('demotes the always-allow family on high risk and makes Deny the default', () => {
    const l = layoutChoices(prompt('high', 'x'));
    // Deny comes BEFORE Allow once on high risk.
    expect(l.visible.map((c) => c.id)).toEqual(['deny', 'allow_once']);
    expect(l.more.map((c) => c.id)).toEqual(['allow_all']);
    expect(l.focusId).toBe('deny');
    expect(l.primaryId).toBe('deny');
  });

  it('falls back to the wording when the backend does not flag choices', () => {
    expect(isBroadChoice({ id: 'yes-always', label: 'Yes, and do not ask again for npm test' })).toBe(true);
    expect(isBroadChoice({ id: 'yes', label: 'Yes' })).toBe(false);
    expect(isBroadChoice({ id: 'no', label: 'No, tell Claude what to do differently' })).toBe(false);
  });
});

describe('ApprovalPrompt', () => {
  it('low risk: headline, one reason, Allow is the primary button, nothing hidden', () => {
    render(prompt('low', 'Run tests'));
    expect(host.querySelector('.ib-title')?.textContent).toBe('Run tests');
    expect(host.querySelector('.ib-reason')?.textContent).toBe('It deletes ../old and everything in it.');
    expect(host.querySelector('.ib-caution')).toBeNull();
    expect(btn('More options')).toBeUndefined();
    expect(btn('Allow once')?.className).toContain('ck-btn--primary');
    expect(btn('Always allow')).toBeDefined();
    // low risk keeps the agent's order: Allow once, Always allow, Deny
    expect(Array.from(host.querySelectorAll('[data-ib="answer"]')).map((b) => b.textContent)).toEqual(['Allow once', 'Always allow', 'Deny']);
  });

  it('high risk: caution line, Always allow behind More options, focus on Deny not Allow', () => {
    render(prompt('high', 'Delete a folder outside this project'));
    expect(host.querySelector('.ib-caution')?.textContent).toMatch(/hard to undo/);
    expect(btn('Always allow')).toBeUndefined();
    expect(btn('More options')).toBeDefined();
    expect(document.activeElement).toBe(btn('Deny'));
    // the focus ring is drawn for focus that came from code
    expect(btn('Deny')?.hasAttribute('data-ring')).toBe(true);
    // and Deny leads the row
    expect(Array.from(host.querySelectorAll('[data-ib="answer"]')).map((b) => b.textContent)).toEqual(['Deny', 'Allow once']);
    expect(host.querySelector('[data-ib-card]')?.hasAttribute('data-guard')).toBe(true);
    expect(document.activeElement).not.toBe(btn('Allow once'));
    expect(btn('Allow once')?.className).not.toContain('ck-btn--primary');
    expect(btn('Deny')?.className).toContain('ck-btn--primary');
    // the ring goes when focus leaves
    act(() => btn('Allow once')!.focus());
    expect(btn('Deny')?.hasAttribute('data-ring')).toBe(false);
  });

  it('high risk: the demoted choices are still reachable and answer normally', () => {
    const onAnswer = render(prompt('high', 'Delete a folder outside this project'));
    act(() => btn('More options')!.click());
    expect(btn('Always allow')).toBeDefined();
    act(() => btn('Always allow')!.click());
    expect(onAnswer).toHaveBeenCalledWith(expect.objectContaining({ id: 'allow_all' }), expect.objectContaining({ kind: 'click' }));
  });

  it('every answer carries the click that chose it: a fresh gesture per click', () => {
    const onAnswer = render(prompt('medium', 'Delete a folder'));
    act(() => btn('Deny')!.click());
    act(() => btn('Deny')!.click());
    expect(onAnswer).toHaveBeenCalledTimes(2);
    const [first, second] = onAnswer.mock.calls.map((call) => call[1]);
    expect(first).toMatchObject({ kind: 'click', trusted: false });
    expect(typeof first.atMs).toBe('number');
    expect(first.id).not.toBe(second.id);
  });

  it('nothing answers without a click: mounting, re-rendering and focusing send nothing', () => {
    const onAnswer = render(prompt('high', 'Delete a folder outside this project'));
    act(() => root.render(<ApprovalPrompt prompt={prompt('high', 'Delete a folder outside this project')} onAnswer={onAnswer} variant="card" />));
    act(() => root.render(<ApprovalPrompt prompt={prompt('medium', 'Trust this folder')} onAnswer={onAnswer} variant="card" />));
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it('a click on Allow once still works on a high-risk prompt (explicit action only)', () => {
    const onAnswer = render(prompt('high', 'x'));
    expect(onAnswer).not.toHaveBeenCalled();
    act(() => btn('Allow once')!.click());
    expect(onAnswer).toHaveBeenCalledTimes(1);
  });

  it('Show details expands the full command and every reason', () => {
    render(prompt('medium', 'Delete a folder'));
    expect(host.querySelector('.ib-cmd')).toBeNull();
    act(() => btn('Details')!.click());
    expect(host.querySelector('.ib-cmd')?.textContent).toBe('Run command: rm -rf ../old');
    // the first reason is already on the card; Details adds only what it did not say
    const reasons = Array.from(host.querySelectorAll('.ib-details li')).map((li) => li.textContent);
    expect(reasons).toEqual(['That is outside this project.']);
    expect(host.querySelector('.ib-details')?.textContent).not.toContain('It deletes ../old and everything in it.');
    expect(btn('Hide details')).toBeDefined();
  });

  it('does not steal focus from a text field', () => {
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();
    render(prompt('high', 'x'));
    expect(document.activeElement).toBe(input);
    input.remove();
  });

  it('is monochrome: no inline colours', () => {
    render(prompt('high', 'x'));
    expect(host.innerHTML).not.toMatch(/#[0-9a-f]{3,6}\b|rgb\(|red/i);
  });

  it('says the same words whatever the agent menu says, and keeps the agent words in the tooltip', () => {
    const p: PanePrompt = {
      ...prompt('low', 'Run tests'),
      choices: [
        { id: 'yes', label: 'Yes' },
        { id: 'yes-always', label: 'Yes, and do not ask again for npm test' },
        { id: 'no', label: 'No, tell Claude what to do differently' },
      ],
    };
    render(p);
    const answers = Array.from(host.querySelectorAll<HTMLElement>('[data-ib="answer"]'));
    expect(answers.map((b) => b.textContent)).toEqual(['Allow once', 'Always allow npm test', 'Deny']);
    // the command is inline code, never backticks
    expect(answers[1].querySelector('code')?.textContent).toBe('npm test');
    expect(host.textContent).not.toContain('`');
    expect(answers[2].getAttribute('title')).toBe('No, tell Claude what to do differently');
  });

  it('a folder-trust question keeps its own words (Trust this folder, Don’t trust), never Allow once', () => {
    // Live (integration v0.2.1): the Codex trust card read "Allow once / Deny".
    const trust: PanePrompt = {
      promptId: 'p-trust',
      kind: 'workspace_trust',
      text: 'raw',
      summary: 'Trust this folder?',
      choices: [{ id: 'trust', label: 'Trust this folder' }, { id: 'deny', label: "Don't trust" }],
      risk: { level: 'medium', headline: 'Trust this folder', reasons: ['The agent will be able to read and change files here.'] },
    };
    const onAnswer = render(trust);
    expect(btn('Allow once')).toBeUndefined();
    expect(btn('Deny')).toBeUndefined();
    act(() => btn('Trust this folder')!.click());
    expect(onAnswer).toHaveBeenCalledWith({ id: 'trust', label: 'Trust this folder' }, expect.anything());
    expect(btn("Don't trust")).toBeTruthy();
  });

  it('answers with the agent choice, not the renamed word', () => {
    const p: PanePrompt = { ...prompt('low', 'Run tests'), choices: [{ id: 'yes', label: 'Yes' }, { id: 'no', label: 'No, and tell Codex what to do differently (esc)' }] };
    const onAnswer = render(p);
    act(() => btn('Deny')!.click());
    expect(onAnswer).toHaveBeenCalledWith({ id: 'no', label: 'No, and tell Codex what to do differently (esc)' }, expect.objectContaining({ kind: 'click' }));
  });

  it('the card carries the agent, the project, the risk word and one age', () => {
    act(() => root.render(<ApprovalPrompt prompt={prompt('high', 'Delete a folder')} onAnswer={vi.fn()} variant="card" who="Codex · billing-service" age={125_000} />));
    expect(host.querySelector('.ib-meta')?.textContent).toBe('Codex · billing-service');
    expect(host.querySelector('.ib-status')?.textContent).toBe(LEVEL_WORD.high);
    expect(host.querySelector('.ib-age')?.textContent).toBe('2m');
  });

  it('the docked bar uses the same words and the same order', () => {
    act(() => root.render(<ApprovalPrompt prompt={prompt('high', 'x')} onAnswer={vi.fn()} variant="bar" />));
    expect(Array.from(host.querySelectorAll('[data-ib="answer"]')).map((b) => b.textContent)).toEqual(['Deny', 'Allow once']);
  });
});

describe('choiceWords', () => {
  const words = (choices: { id: string; label: string; broad?: boolean }[]) => Object.fromEntries([...choiceWords(choices)].map(([id, w]) => [id, w.code ? `${w.text} ${w.code}` : w.text]));

  it('maps Claude and Codex menus onto Allow once / Always allow <command> / Deny', () => {
    expect(
      words([
        { id: 'a', label: 'Yes' },
        { id: 'b', label: 'Yes, and do not ask again for npm test' },
        { id: 'c', label: 'No, tell Claude what to do differently' },
      ]),
    ).toEqual({ a: 'Allow once', b: 'Always allow npm test', c: 'Deny' });
    expect(
      words([
        { id: 'a', label: 'Yes, proceed (y)' },
        { id: 'b', label: "Yes, and don't ask again for commands that start with `git status` (p)" },
        { id: 'c', label: 'No, and tell Codex what to do differently (esc)' },
      ]),
    ).toEqual({ a: 'Allow once', b: 'Always allow commands starting with git status', c: 'Deny' });
  });

  it('does not flatten a scope it cannot name: it keeps the agent words (or says "for this session")', () => {
    expect(words([{ id: 'a', label: 'Yes' }, { id: 'b', label: 'Yes, allow all edits during this session (shift+tab)' }])).toEqual({ a: 'Allow once', b: 'Allow for this session' });
    expect(words([{ id: 'a', label: 'Yes' }, { id: 'b', label: 'Yes, and always trust this folder' }, { id: 'c', label: 'Yes, but only the first file' }])).toEqual({
      a: 'Allow once',
      b: 'Yes, and always trust this folder',
      c: 'Yes, but only the first file',
    });
  });

  it('renames only the first no; a second one keeps its words', () => {
    expect(words([{ id: 'a', label: 'No' }, { id: 'b', label: 'Cancel the whole task' }])).toEqual({ a: 'Deny', b: 'Cancel the whole task' });
  });
});

describe('risk words', () => {
  it('are the words the Rust side uses (attention::model Level::word), so desktop, phone and push agree', () => {
    const rust = readFileSync(resolve(__dirname, '../../../src-tauri/src/attention/model.rs'), 'utf8');
    for (const word of Object.values(LEVEL_WORD)) expect(rust).toContain(`"${word}"`);
  });
});

describe('the agent\'s own question', () => {
  const question: PanePrompt = {
    promptId: 'q-1',
    kind: 'question',
    text: 'Pick a colour\nRed\nBlue',
    summary: 'Pick a colour',
    choices: [
      { id: '1', label: 'Red' },
      { id: '2', label: 'Blue' },
    ],
    risk: { level: 'low', headline: 'The agent has a question', reasons: ['Pick the answer you want. Nothing is changed until you do.'] },
  };

  it('keeps the agent\'s words on the buttons and shows no risk word', () => {
    const onAnswer = render(question);
    expect(btn('Red')).toBeTruthy();
    expect(btn('Blue')).toBeTruthy();
    expect(btn('Allow once')).toBeUndefined();
    expect(host.textContent).not.toContain(LEVEL_WORD.low);
    // The question itself is the card's title, not a risk headline.
    expect(host.querySelector('.ib-title, h3, [class*=title]')?.textContent ?? host.textContent).toContain('Pick a colour');
    act(() => btn('Blue')!.click());
    expect(onAnswer).toHaveBeenCalledTimes(1);
    expect(onAnswer.mock.calls[0][0]).toMatchObject({ id: '2', label: 'Blue' });
  });
});
