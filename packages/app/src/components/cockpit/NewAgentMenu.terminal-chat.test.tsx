// @vitest-environment jsdom
// The "+" menu's three-way switch: Terminal | Chat | Chat (terminal). Chat (terminal) starts through `onStartTerminal`
// (the caller runs terminal_chat_start for that agent) and never through the ACP start. A failed start says why, with
// no "Start in terminal" button, because a Chat (terminal) pane already is a terminal.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpLaneAgent, TerminalChatAgent } from '../../lib/acpChat';
import type { AccountsApi } from '../../lib/accounts';
import { NewAgentMenu } from './NewAgentMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ENTRIES = [
  { type: 'claude', label: 'Claude' },
  { type: 'codex', label: 'Codex' },
];
const accounts = { list: async () => ({ agents: [], accounts: [] }) } as unknown as AccountsApi;
const tc = (over: Partial<TerminalChatAgent> = {}): TerminalChatAgent => ({
  id: 'codex',
  label: 'Codex',
  reads_file: true,
  can_resume: false,
  can_answer_prompts: false,
  note: 'It cannot resume old sessions.',
  chosen: false,
  ...over,
});
const acp = (over: Partial<AcpLaneAgent> = {}): AcpLaneAgent => ({
  id: 'codex',
  label: 'Codex',
  lane: 'terminal',
  effective_lane: 'terminal',
  disabled: false,
  disabled_note: null,
  ...over,
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

type Chat = { agents: AcpLaneAgent[]; onStart: (id: string) => Promise<void>; terminalChat?: TerminalChatAgent[]; onStartTerminal?: (id: string) => Promise<void> };
async function mount(chat: Chat | undefined) {
  const onStart = vi.fn();
  const onClose = vi.fn();
  await act(async () => {
    root.render(<NewAgentMenu entries={ENTRIES} readyAgents={new Set()} onStart={onStart} onClose={onClose} accountsApi={accounts} chat={chat} />);
  });
  return { onStart, onClose };
}

const row = (id: string) => host.querySelector<HTMLElement>(`[data-testid="lane-row-${id}"]`);
const seg = (id: string) => [...row(id)!.querySelectorAll<HTMLButtonElement>('.ck-lane-seg-btn')].map((b) => b.textContent);
const pick = (id: string, label: string) => [...row(id)!.querySelectorAll<HTMLButtonElement>('.ck-lane-seg-btn')].find((b) => b.textContent === label)!;
const main = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button.ck-menu-item')].find((b) => b.textContent?.includes(label))!;

describe('NewAgentMenu three-way switch (Chat (terminal))', () => {
  it('shows Terminal and Chat (terminal) for an agent with no ACP chat', async () => {
    await mount({ agents: [], onStart: vi.fn(), terminalChat: [tc()], onStartTerminal: vi.fn() });
    expect(seg('codex')).toEqual(['Terminal', 'Chat (terminal)']);
    expect(row('claude')).toBeNull();
    expect(host.textContent).not.toContain('ACP');
  });

  it('shows all three values when the agent also has an ACP chat, in order', async () => {
    await mount({ agents: [acp()], onStart: vi.fn(), terminalChat: [tc()], onStartTerminal: vi.fn() });
    expect(seg('codex')).toEqual(['Terminal', 'Chat', 'Chat (terminal)']);
  });

  it("starts from the saved choice: Chat (terminal) when it is the agent's own setting", async () => {
    await mount({ agents: [], onStart: vi.fn(), terminalChat: [tc({ chosen: true })], onStartTerminal: vi.fn() });
    expect(pick('codex', 'Chat (terminal)').getAttribute('aria-pressed')).toBe('true');
    expect(pick('codex', 'Terminal').getAttribute('aria-pressed')).toBe('false');
  });

  it('choosing Chat (terminal) and clicking the agent calls onStartTerminal with that agent, and nothing else', async () => {
    const startTerminal = vi.fn(async () => undefined);
    const chatStart = vi.fn(async () => undefined);
    const { onStart, onClose } = await mount({ agents: [], onStart: chatStart, terminalChat: [tc()], onStartTerminal: startTerminal });
    await act(async () => pick('codex', 'Chat (terminal)').click());
    expect(startTerminal).not.toHaveBeenCalled();
    await act(async () => main('Codex').click());
    expect(startTerminal).toHaveBeenCalledWith('codex');
    expect(chatStart).not.toHaveBeenCalled();
    expect(onStart).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it('Terminal still starts the agent the usual way', async () => {
    const startTerminal = vi.fn(async () => undefined);
    const { onStart } = await mount({ agents: [], onStart: vi.fn(), terminalChat: [tc()], onStartTerminal: startTerminal });
    await act(async () => main('Codex').click());
    expect(onStart).toHaveBeenCalledWith('codex', undefined);
    expect(startTerminal).not.toHaveBeenCalled();
  });

  it('a failed Chat (terminal) start says why and offers no terminal fallback', async () => {
    const startTerminal = vi.fn(async () => {
      throw 'NOT_AVAILABLE: This agent cannot run as a chat in a terminal.';
    });
    const { onClose } = await mount({ agents: [], onStart: vi.fn(), terminalChat: [tc({ chosen: true })], onStartTerminal: startTerminal });
    await act(async () => main('Codex').click());
    expect(onClose).not.toHaveBeenCalled();
    const fail = host.querySelector('[data-testid="chat-start-failed"]')!;
    expect(fail.textContent).toContain('cannot run as a chat in a terminal');
    expect(fail.querySelector('button')).toBeNull();
  });

  it('draws no third value when the agent is not in the terminal chat list', async () => {
    await mount({ agents: [acp()], onStart: vi.fn(), terminalChat: [], onStartTerminal: vi.fn() });
    expect(seg('codex')).toEqual(['Terminal', 'Chat']);
  });
});
