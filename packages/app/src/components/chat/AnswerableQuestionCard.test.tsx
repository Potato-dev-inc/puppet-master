// @vitest-environment jsdom
// The answerable card for a Chat (terminal) pane: the agent's own options as buttons, keys 1-3, a gesture on every
// answer, a stale answer that re-reads the question, nothing when no question is on the screen, and the read-only card
// for an agent that cannot answer prompts.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const readPrompt = vi.fn();
const answer = vi.fn<(pane: string, promptId: string, optionId: string, gesture: unknown) => Promise<void>>(async () => undefined);
vi.mock('../../lib/terminalChatPrompt', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/terminalChatPrompt')>()),
  readTerminalChatPrompt: (pane: string) => readPrompt(pane),
  answerTerminalChatPrompt: (pane: string, promptId: string, optionId: string, gesture: unknown) => answer(pane, promptId, optionId, gesture),
}));
const readSnapshot = vi.fn(async () => '');
vi.mock('../../lib/tauri', () => ({ tauri: { readSnapshot: () => readSnapshot(), writeInput: vi.fn(async () => undefined) } }));

import { TerminalChatAnswerError } from '../../lib/terminalChatPrompt';
import { AnswerableQuestionCard } from './AnswerableQuestionCard';
import { ScreenQuestionCard } from './ScreenQuestionCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TRUST = {
  promptId: 'tcp-1',
  text: 'Do you trust the files in this folder?',
  kind: 'trust' as const,
  rawKind: 'workspace_trust',
  options: [
    { id: 'yes', name: 'Yes, trust this folder' },
    { id: 'no', name: 'No, exit' },
  ],
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  readPrompt.mockReset();
  answer.mockReset();
  answer.mockImplementation(async () => undefined);
  readSnapshot.mockReset();
  readSnapshot.mockImplementation(async () => '');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
const buttons = () => [...host.querySelectorAll<HTMLButtonElement>('button[data-option]')];

describe('AnswerableQuestionCard', () => {
  it('shows the title by kind, the agent text, and one button per option with its own name and no default', async () => {
    readPrompt.mockResolvedValue(TRUST);
    await act(async () => root.render(<AnswerableQuestionCard paneId="p1" agent="Codex" />));
    await flush();
    expect(host.textContent).toContain('Codex asks to trust this folder');
    expect(host.textContent).toContain('Do you trust the files in this folder?');
    expect(buttons().map((b) => b.textContent?.replace(/\s+/g, ' ').trim())).toEqual(['1 Yes, trust this folder', '2 No, exit']);
    expect(buttons().every((b) => b.className.includes('ck-btn--secondary'))).toBe(true);
    expect(answer).not.toHaveBeenCalled();
  });

  it('answers with the exact pane, prompt and option ids and the person click', async () => {
    readPrompt.mockResolvedValue(TRUST);
    await act(async () => root.render(<AnswerableQuestionCard paneId="p1" agent="Codex" />));
    await flush();
    await act(async () => buttons()[1].click());
    expect(answer).toHaveBeenCalledTimes(1);
    const [pane, promptId, optionId, gesture] = answer.mock.calls[0];
    expect([pane, promptId, optionId]).toEqual(['p1', 'tcp-1', 'no']);
    expect(gesture).toMatchObject({ kind: 'click' });
  });

  it('keys 1, 2 and 3 press the first three options; other keys do nothing', async () => {
    readPrompt.mockResolvedValue({ ...TRUST, options: [...TRUST.options, { id: 'c', name: 'Third' }, { id: 'd', name: 'Fourth' }] });
    await act(async () => root.render(<AnswerableQuestionCard paneId="p1" agent="Codex" />));
    await flush();
    // Focus inside the card: the keys answer only when the person is in this card or its pane.
    buttons()[0].focus();
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    });
    expect(answer).toHaveBeenCalledTimes(1);
    expect(answer.mock.calls[0][2]).toBe('no');
    expect(answer.mock.calls[0][3]).toMatchObject({ kind: 'keydown' });
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '3' }));
    });
    expect(answer.mock.calls[1][2]).toBe('c');
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '4' }));
    });
    expect(answer).toHaveBeenCalledTimes(2);
  });

  it('a stale answer re-reads the question and shows the new one with a note', async () => {
    readPrompt.mockResolvedValueOnce(TRUST).mockResolvedValue({ ...TRUST, promptId: 'tcp-2', text: 'Allow this command?', kind: 'permission', options: [{ id: 'once', name: 'Yes, once' }] });
    answer.mockRejectedValueOnce(new TerminalChatAnswerError('TERMINAL_CHAT_PROMPT_STALE', 'changed'));
    await act(async () => root.render(<AnswerableQuestionCard paneId="p1" agent="Codex" />));
    await flush();
    await act(async () => buttons()[0].click());
    await flush();
    expect(host.textContent).toContain('The question changed. Here is the new one.');
    expect(host.textContent).toContain('Codex asks to run something');
    expect(host.textContent).toContain('Allow this command?');
    expect(answer).toHaveBeenCalledTimes(1);
  });

  it('hides itself when no question is on the screen', async () => {
    readPrompt.mockResolvedValue(null);
    await act(async () => root.render(<AnswerableQuestionCard paneId="p1" agent="Codex" />));
    await flush();
    expect(host.innerHTML).toBe('');
  });

  it('shows nothing while covered by another card', async () => {
    readPrompt.mockResolvedValue(TRUST);
    await act(async () => root.render(<AnswerableQuestionCard paneId="p1" agent="Codex" covered />));
    await flush();
    expect(readPrompt).not.toHaveBeenCalled();
    expect(host.innerHTML).toBe('');
  });

  it('an agent that cannot answer keeps the read-only card', async () => {
    readPrompt.mockResolvedValue(TRUST);
    readSnapshot.mockResolvedValue(' Login\n Select login method:\n\n ❯ 1. Account\n   2. Console\n');
    await act(async () => root.render(<ScreenQuestionCard paneId="p1" waiting idle={false} />));
    await flush();
    expect(host.querySelector('[data-screen-question]')).not.toBeNull();
    expect(host.querySelector('[data-answerable-question]')).toBeNull();
    expect(readPrompt).not.toHaveBeenCalled();
  });

  it('with an answering agent the wrapper draws the answerable card', async () => {
    readPrompt.mockResolvedValue(TRUST);
    await act(async () => root.render(<ScreenQuestionCard paneId="p1" waiting answerable="Codex" />));
    await flush();
    expect(host.querySelector('[data-answerable-question]')).not.toBeNull();
    expect(host.textContent).not.toContain('ACP');
  });

  it('split view: a key answers only the pane that has focus; body focus answers nothing', async () => {
    readPrompt.mockResolvedValue(TRUST);
    const extra = document.createElement('button');
    extra.textContent = 'pane A control';
    await act(async () => {
      root.render(
        <>
          <div data-pane-id="pA">
            <AnswerableQuestionCard paneId="pA" agent="Codex" />
          </div>
          <div data-pane-id="pB">
            <AnswerableQuestionCard paneId="pB" agent="Claude" />
          </div>
        </>,
      );
    });
    await flush();
    host.querySelector('[data-pane-id="pA"]')!.appendChild(extra);
    extra.focus();
    expect(document.activeElement).toBe(extra);
    await act(async () => {
      extra.dispatchEvent(new KeyboardEvent('keydown', { key: '1', bubbles: true }));
    });
    expect(answer).toHaveBeenCalledTimes(1);
    expect(answer.mock.calls[0][0]).toBe('pA');

    (document.activeElement as HTMLElement | null)?.blur();
    expect(document.activeElement === document.body || document.activeElement === null).toBe(true);
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    });
    expect(answer).toHaveBeenCalledTimes(1);
    extra.remove();
  });
});
