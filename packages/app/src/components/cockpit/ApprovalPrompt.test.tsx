// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PanePrompt } from '../../lib/cockpit-api';
import { isBroadChoice, layoutChoices } from '../../lib/approval-risk';
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
    expect(l.visible.map((c) => c.id)).toEqual(['allow_once', 'deny']);
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
    expect(host.querySelector('.ck-appr-headline')?.textContent).toBe('Run tests');
    expect(host.querySelector('.ck-appr-reason')?.textContent).toBe('It deletes ../old and everything in it.');
    expect(host.querySelector('.ck-appr-caution')).toBeNull();
    expect(btn('More options')).toBeUndefined();
    expect(btn('Allow once')?.className).toContain('ck-btn--primary');
    expect(btn('Always allow')).toBeDefined();
  });

  it('high risk: caution line, Always allow behind More options, focus on Deny not Allow', () => {
    render(prompt('high', 'Delete a folder outside this project'));
    expect(host.querySelector('.ck-appr-caution')?.textContent).toMatch(/hard to undo/);
    expect(btn('Always allow')).toBeUndefined();
    expect(btn('More options')).toBeDefined();
    expect(document.activeElement).toBe(btn('Deny'));
    expect(document.activeElement).not.toBe(btn('Allow once'));
    expect(btn('Allow once')?.className).not.toContain('ck-btn--primary');
    expect(btn('Deny')?.className).toContain('ck-btn--primary');
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
    expect(host.querySelector('.ck-appr-cmd')).toBeNull();
    act(() => btn('Show details')!.click());
    expect(host.querySelector('.ck-appr-cmd')?.textContent).toBe('Run command: rm -rf ../old');
    expect(host.querySelectorAll('.ck-appr-details li')).toHaveLength(2);
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
});
