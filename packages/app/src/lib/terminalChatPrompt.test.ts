import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { answerTerminalChatPrompt, parseTerminalChatError, readTerminalChatPrompt } from './terminalChatPrompt';

const click = { id: 'g1', trusted: true, atMs: 1, kind: 'click' };

describe('terminal chat prompt wrappers', () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it('reads the question for a pane', async () => {
    invoke.mockResolvedValue(null);
    expect(await readTerminalChatPrompt('p1')).toBeNull();
    expect(invoke).toHaveBeenCalledWith('terminal_chat_prompt', { pane: 'p1' });
  });

  it('sends exactly the ids and the click', async () => {
    invoke.mockResolvedValue(undefined);
    await answerTerminalChatPrompt('p1', 'tcp-1', 'allow_once', click);
    expect(invoke).toHaveBeenCalledWith('terminal_chat_answer', { pane: 'p1', promptId: 'tcp-1', optionId: 'allow_once', gesture: click });
  });

  it('turns a stale refusal into an error the card can act on', async () => {
    invoke.mockImplementation(async () => {
      throw new Error('TERMINAL_CHAT_PROMPT_STALE: the question on the screen changed; nothing was sent');
    });
    const error = await answerTerminalChatPrompt('p1', 'tcp-1', 'deny', click).catch((e) => e);
    expect(error.isStale).toBe(true);
    expect(error.code).toBe('TERMINAL_CHAT_PROMPT_STALE');
  });

  it('keeps unknown errors as text', () => {
    const error = parseTerminalChatError(new Error('boom'));
    expect(error.code).toBe('UNKNOWN');
    expect(error.isStale).toBe(false);
  });
});
