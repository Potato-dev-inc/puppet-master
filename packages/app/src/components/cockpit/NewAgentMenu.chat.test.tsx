// @vitest-environment jsdom
// The "+" menu with chat sessions (ACP): a Terminal | Chat switch on the row of an agent that can run as a chat, set to
// the agent's own setting. Chat starts a chat session, Terminal starts as before; a failed chat start says why and,
// when the terminal can run the agent, offers "Start in terminal". Without the `chat` prop the menu is unchanged.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpLaneAgent } from '../../lib/acpChat';
import type { AccountsApi } from '../../lib/accounts';
import { NewAgentMenu } from './NewAgentMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ENTRIES = [
  { type: 'claude', label: 'Claude' },
  { type: 'codex', label: 'Codex' },
];
const accounts = { list: async () => ({ agents: [], accounts: [] }) } as unknown as AccountsApi;
const agent = (over: Partial<AcpLaneAgent> = {}): AcpLaneAgent => ({
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

async function mount(chat: { agents: AcpLaneAgent[]; onStart: (id: string) => Promise<void> } | undefined) {
  const onStart = vi.fn();
  const onClose = vi.fn();
  await act(async () => {
    root.render(<NewAgentMenu entries={ENTRIES} readyAgents={new Set()} onStart={onStart} onClose={onClose} accountsApi={accounts} chat={chat} />);
  });
  return { onStart, onClose };
}

const row = (id: string) => host.querySelector<HTMLElement>(`[data-testid="lane-row-${id}"]`);
const pick = (id: string, label: string) => [...row(id)!.querySelectorAll<HTMLButtonElement>('.ck-lane-seg-btn')].find((b) => b.textContent === label)!;
const main = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button.ck-menu-item')].find((b) => b.textContent?.includes(label))!;

describe('NewAgentMenu with chat sessions', () => {
  it('draws no switch without the chat prop, and the agent starts as before', async () => {
    const { onStart, onClose } = await mount(undefined);
    expect(host.querySelector('.ck-lane-seg')).toBeNull();
    await act(async () => main('Codex').click());
    expect(onStart).toHaveBeenCalledWith('codex', undefined);
    expect(onClose).toHaveBeenCalled();
  });

  it('draws the switch only on the agent that can run as a chat, set to its own setting', async () => {
    await mount({ agents: [agent({ lane: 'acp', effective_lane: 'acp' })], onStart: vi.fn(async () => undefined) });
    expect(row('claude')).toBeNull();
    expect(row('codex')).not.toBeNull();
    expect(pick('codex', 'Chat').getAttribute('aria-pressed')).toBe('true');
    expect(pick('codex', 'Terminal').getAttribute('aria-pressed')).toBe('false');
  });

  it('defaults to Terminal, and Terminal starts the agent the usual way', async () => {
    const chatStart = vi.fn(async () => undefined);
    const { onStart } = await mount({ agents: [agent()], onStart: chatStart });
    expect(pick('codex', 'Terminal').getAttribute('aria-pressed')).toBe('true');
    await act(async () => main('Codex').click());
    expect(onStart).toHaveBeenCalledWith('codex', undefined);
    expect(chatStart).not.toHaveBeenCalled();
  });

  it('picking Chat starts a chat session and closes the menu; the switch does not start anything by itself', async () => {
    const chatStart = vi.fn(async () => undefined);
    const { onStart, onClose } = await mount({ agents: [agent()], onStart: chatStart });
    await act(async () => pick('codex', 'Chat').click());
    expect(chatStart).not.toHaveBeenCalled();
    expect(pick('codex', 'Chat').getAttribute('aria-pressed')).toBe('true');
    await act(async () => main('Codex').click());
    expect(chatStart).toHaveBeenCalledWith('codex');
    expect(onStart).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it('a failed chat start says why and offers the terminal when it can run the agent', async () => {
    const chatStart = vi.fn(async () => {
      throw 'ACP_ADAPTER_MISSING: The chat could not start. The terminal can run this agent instead.';
    });
    const { onStart, onClose } = await mount({ agents: [agent({ lane: 'acp', effective_lane: 'acp' })], onStart: chatStart });
    await act(async () => main('Codex').click());
    expect(onClose).not.toHaveBeenCalled();
    const fail = host.querySelector('[data-testid="chat-start-failed"]')!;
    expect(fail.textContent).toContain('The chat could not start. The terminal can run this agent instead.');
    expect(fail.textContent).not.toContain('ACP_ADAPTER_MISSING');
    const fallback = [...fail.querySelectorAll('button')].find((b) => b.textContent === 'Start in terminal')!;
    await act(async () => fallback.click());
    expect(onStart).toHaveBeenCalledWith('codex', undefined);
    expect(onClose).toHaveBeenCalled();
  });

  it('a failed start with no terminal fallback only says why', async () => {
    const chatStart = vi.fn(async () => {
      throw new Error('Pick a project folder first.');
    });
    await mount({ agents: [agent({ lane: 'acp', effective_lane: 'acp' })], onStart: chatStart });
    await act(async () => main('Codex').click());
    const fail = host.querySelector('[data-testid="chat-start-failed"]')!;
    expect(fail.textContent).toBe('Pick a project folder first.');
    expect(fail.querySelector('button')).toBeNull();
  });
});
