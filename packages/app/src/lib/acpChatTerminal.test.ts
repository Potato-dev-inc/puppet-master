// @vitest-environment jsdom
// The Chat (terminal) helpers: the start command's arguments, the lane check on a pane, and which agents the menus may offer.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpLaneView } from './acpChat';
import { isTerminalChatLane, startTerminalChat, terminalChatCapable, TERMINAL_CHAT_BADGE } from './acpChat';

const invoke = vi.fn(async (..._args: unknown[]) => 'pane-7');
vi.mock('@tauri-apps/api/core', () => ({ invoke: (cmd: string, args: unknown) => invoke(cmd, args) }));

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockClear();
});
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
});

describe('Chat (terminal) helpers', () => {
  it('startTerminalChat calls terminal_chat_start with the agent and the project folder, and returns the pane id', async () => {
    await expect(startTerminalChat('codex', 'D:\\work\\shop')).resolves.toBe('pane-7');
    expect(invoke).toHaveBeenCalledWith('terminal_chat_start', { agent: 'codex', cwd: 'D:\\work\\shop' });
  });

  it('startTerminalChat sends no folder as null', async () => {
    await startTerminalChat('codex');
    expect(invoke).toHaveBeenCalledWith('terminal_chat_start', { agent: 'codex', cwd: null });
  });

  it('recognises only the terminal_chat lane on a pane', () => {
    expect(isTerminalChatLane('terminal_chat')).toBe(true);
    expect(isTerminalChatLane('acp')).toBe(false);
    expect(isTerminalChatLane(undefined)).toBe(false);
    expect(isTerminalChatLane(null)).toBe(false);
  });

  it('the badge is the plain words, never ACP', () => {
    expect(TERMINAL_CHAT_BADGE).toBe('Chat (terminal)');
    expect(TERMINAL_CHAT_BADGE).not.toContain('ACP');
  });

  it('offers Chat (terminal) agents only while Labs is on, and an older backend gives none', () => {
    const agent = { id: 'codex', label: 'Codex', reads_file: true, can_resume: true, can_answer_prompts: false, note: '', chosen: false };
    const on: AcpLaneView = { labs_enabled: true, agents: [], terminal_chat: [agent] };
    expect(terminalChatCapable(on)).toEqual([agent]);
    expect(terminalChatCapable({ ...on, labs_enabled: false })).toEqual([]);
    expect(terminalChatCapable({ labs_enabled: true, agents: [] })).toEqual([]);
    expect(terminalChatCapable(null)).toEqual([]);
  });
});
