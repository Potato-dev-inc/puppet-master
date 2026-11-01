// @vitest-environment jsdom
// "Add chat (ACP)": detect -> best guess -> confirm (no typing) -> set with confirmed true; no guess / error / timeout -> form; a refusal shows the backend's text; the
// Settings rows (add / remove / reason) and the "+" menu hint (only for agents that can get a chat, Labs on).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpLaneAgent, AcpLaneView, OverlayDetect } from '../../../lib/acpChat';
import { addableChat, normalizeLaneView } from '../../../lib/acpChat';
import { AddChatDialog, commandLine, confirmSentence, DETECT_TIMEOUT_MS } from './AddChatDialog';
import { AgentLaneRows } from './AgentLaneRows';
import { NewAgentMenu } from '../../cockpit/NewAgentMenu';
import type { AccountsApi } from '../../../lib/accounts';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const detected: OverlayDetect = {
  ok: true,
  command: 'fake-agent',
  candidates: [
    { command: 'fake-agent', args: ['acp', '--quiet'], evidence: 'acp  Start the chat server', confidence: 'high' },
    { command: 'fake-bridge', args: ['--stdio'], evidence: 'fake-bridge  over stdio', confidence: 'medium' },
  ],
  stdio_hint: null,
  mentions_protocol: true,
};
const row = (over: Partial<AcpLaneAgent> = {}): AcpLaneAgent => ({ id: 'fake', label: 'Fake', lane: 'terminal', effective_lane: 'terminal', disabled: false, disabled_note: null, ...over });

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

const q = <T extends Element = HTMLElement>(sel: string) => document.querySelector<T>(sel);
const byText = (text: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text);
const click = async (el: Element | null | undefined) => {
  expect(el, 'element to click').toBeTruthy();
  await act(async () => (el as HTMLElement).click());
};
const flush = async () => act(async () => void (await Promise.resolve()));

async function open(api: Parameters<typeof AddChatDialog>[0]['api'], onAdded = vi.fn(), onClose = vi.fn()) {
  await act(async () => root.render(<AddChatDialog agent={{ id: 'fake', label: 'Fake' }} api={api} onClose={onClose} onAdded={onAdded} />));
  await flush();
  return { onAdded, onClose };
}

const setField = async (id: string, value: string) =>
  act(async () => {
    const el = q<HTMLInputElement | HTMLTextAreaElement>(id)!;
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });

describe('AddChatDialog', () => {
  it('detects, picks the best guess and shows the confirm screen at once (nothing to type)', async () => {
    const overlayDetect = vi.fn(async () => ({
      ...detected,
      // the medium one comes first: the high one must still win
      candidates: [detected.candidates[1], detected.candidates[0]],
    }));
    const overlaySet = vi.fn(async () => row({ overlay: true }));
    const { onAdded } = await open({ overlayDetect, overlaySet });
    expect(overlayDetect).toHaveBeenCalledWith('fake');
    expect(overlayDetect).toHaveBeenCalledTimes(1);
    expect(q('[data-testid="add-chat-confirm"]')).toBeTruthy();
    expect(q('[data-testid="acp-step"]')).toBeNull();
    expect(q('[data-testid="add-chat-command"]')?.textContent).toBe('fake-agent acp --quiet');
    expect(q('[data-testid="add-chat-summary"]')?.textContent).toBe('Fake will start fake-agent acp --quiet for chat.');
    expect(document.body.textContent).toContain(confirmSentence('Fake'));
    expect(overlaySet).not.toHaveBeenCalled();
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'fake-agent', args: ['acp', '--quiet'], env_allow: [] }, true);
    expect(onAdded).toHaveBeenCalledTimes(1);
  });

  it('Change details opens the form pre-filled with the guess and Review comes back to confirm', async () => {
    const overlaySet = vi.fn(async () => row());
    await open({ overlayDetect: async () => detected, overlaySet });
    await click(q('[data-testid="add-chat-change"]'));
    expect(q('[data-testid="acp-step"]')).toBeTruthy();
    expect(q<HTMLInputElement>('#acp-command')?.value).toBe('fake-agent');
    expect(q<HTMLTextAreaElement>('#acp-args')?.value).toBe('acp\n--quiet');
    expect(q('[data-testid="add-chat-note"]')).toBeNull();
    await setField('#acp-args', 'serve\n--acp');
    await click(byText('Review'));
    expect(q('[data-testid="add-chat-command"]')?.textContent).toBe('fake-agent serve --acp');
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'fake-agent', args: ['serve', '--acp'], env_allow: [] }, true);
  });

  it('finding nothing says so plainly and shows the form; a typed command works', async () => {
    const overlaySet = vi.fn(async () => row());
    await open({ overlayDetect: async () => ({ ok: true, command: 'x', candidates: [] }), overlaySet });
    expect(q('[data-testid="add-chat-confirm"]')).toBeNull();
    expect(q('[data-testid="add-chat-note"]')?.textContent).toBe('Could not find how Fake offers chat. You can type it in.');
    expect(q('[data-testid="acp-step"]')).toBeTruthy();
    await setField('#acp-command', 'my-agent');
    await setField('#acp-args', 'serve\n--acp');
    await setField('#acp-env', 'MY_KEY');
    await click(byText('Review'));
    expect(commandLine({ command: 'my-agent', args: ['serve', '--acp'], env_allow: [] })).toBe('my-agent serve --acp');
    await click(byText('Add chat'));
    expect(overlaySet).toHaveBeenCalledWith('fake', { command: 'my-agent', args: ['serve', '--acp'], env_allow: ['MY_KEY'] }, true);
  });

  it('a failed look-up shows the message and the form', async () => {
    await open({ overlayDetect: async () => Promise.reject(new Error('WIZARD_NOT_FOUND: `fake` was not found on PATH.')), overlaySet: vi.fn() });
    expect(q('[data-testid="add-chat-reading"]')).toBeNull();
    expect(q('[data-testid="add-chat-note"]')?.textContent).toContain('Could not find how Fake offers chat. You can type it in.');
    expect(q('[data-testid="acp-step"]')).toBeTruthy();
  });

  it('gives up after 20 seconds so "Reading the help text..." can never hang', async () => {
    vi.useFakeTimers();
    try {
      const overlayDetect = vi.fn(() => new Promise<OverlayDetect>(() => {}));
      await open({ overlayDetect, overlaySet: vi.fn() });
      expect(q('[data-testid="add-chat-reading"]')).toBeTruthy();
      await act(async () => void (await vi.advanceTimersByTimeAsync(DETECT_TIMEOUT_MS - 1)));
      expect(q('[data-testid="add-chat-reading"]')).toBeTruthy();
      await act(async () => void (await vi.advanceTimersByTimeAsync(2)));
      expect(q('[data-testid="add-chat-reading"]')).toBeNull();
      expect(q('[data-testid="add-chat-note"]')?.textContent).toContain('You can type it in.');
      expect(q('[data-testid="acp-step"]')).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows the backend error text and stays open when it refuses', async () => {
    const overlaySet = vi.fn(async () => {
      throw new Error('ACP_OVERLAY_REFUSED: That program is a shell, which is not allowed.');
    });
    const { onAdded } = await open({ overlayDetect: async () => detected, overlaySet });
    await click(byText('Add chat'));
    expect(q('[data-testid="add-chat-error"]')?.textContent).toBe('That program is a shell, which is not allowed.');
    expect(onAdded).not.toHaveBeenCalled();
    expect(byText('Add chat')?.disabled).toBe(false);
  });

  it('Cancel on the confirm screen closes without saving', async () => {
    const overlaySet = vi.fn();
    const { onClose } = await open({ overlayDetect: async () => detected, overlaySet });
    await click(byText('Cancel'));
    expect(onClose).toHaveBeenCalled();
    expect(overlaySet).not.toHaveBeenCalled();
  });
});

describe('lane view parsing', () => {
  it('defaults the new fields for an old backend', () => {
    const v = normalizeLaneView({ labs_enabled: true, agents: [row()] });
    expect(v.can_add_chat).toEqual([]);
    expect(addableChat(v)).toEqual([]);
    expect(addableChat({ labs_enabled: false, agents: [], can_add_chat: [{ id: 'a', label: 'A' }] })).toEqual([]);
  });
});

describe('Settings How to run rows', () => {
  const view = (): AcpLaneView => ({
    labs_enabled: true,
    agents: [row({ id: 'mine', label: 'Mine', overlay: true })],
    can_add_chat: [
      { id: 'fake', label: 'Fake' },
      { id: 'blocked', label: 'Blocked', reason: 'It has no help text to read.' },
    ],
  });

  it('add button opens the dialog; a success refreshes the list', async () => {
    let calls = 0;
    const laneGet = vi.fn(async () => (calls++ === 0 ? view() : { labs_enabled: true, agents: [row({ id: 'fake', overlay: true })], can_add_chat: [] }));
    const overlaySet = vi.fn(async () => row({ overlay: true }));
    await act(async () => root.render(<AgentLaneRows api={{ laneGet, laneSet: vi.fn(), overlayDetect: async () => detected, overlaySet, overlayRemove: vi.fn() }} />));
    await flush();
    const blocked = document.querySelector<HTMLButtonElement>('[aria-label="Add chat (ACP) for Blocked"]');
    expect(blocked?.disabled).toBe(true);
    expect(document.body.textContent).toContain('It has no help text to read.');
    await click(document.querySelector('[aria-label="Add chat (ACP) for Fake"]'));
    await flush();
    await click(byText('Add chat'));
    await flush();
    expect(overlaySet).toHaveBeenCalledWith('fake', expect.anything(), true);
    expect(laneGet).toHaveBeenCalledTimes(2);
    expect(q('[data-testid="add-chat-dialog"]')).toBeNull();
    expect(document.querySelector('[aria-label="Add chat (ACP) for Fake"]')).toBeNull();
  });

  it('Remove chat shows only on an overlay agent and calls remove', async () => {
    const overlayRemove = vi.fn(async () => undefined);
    const laneGet = vi.fn(async () => view());
    await act(async () => root.render(<AgentLaneRows api={{ laneGet, laneSet: vi.fn(), overlayRemove }} />));
    await flush();
    expect(document.querySelectorAll('[aria-label^="Remove chat for"]')).toHaveLength(1);
    await click(document.querySelector('[aria-label="Remove chat for Mine"]'));
    expect(overlayRemove).toHaveBeenCalledWith('mine');
    expect(laneGet).toHaveBeenCalledTimes(2);
  });
});

describe('the + menu hint', () => {
  const accounts = { list: async () => ({ agents: [], accounts: [] }) } as unknown as AccountsApi;
  const entries = [
    { type: 'fake', label: 'Fake' },
    { type: 'other', label: 'Other' },
  ];
  async function menu(chat: Parameters<typeof NewAgentMenu>[0]['chat']) {
    await act(async () => root.render(<NewAgentMenu entries={entries} readyAgents={new Set()} onStart={vi.fn()} onClose={vi.fn()} accountsApi={accounts} chat={chat} />));
  }

  it('shows "Set up chat" only next to an agent in can_add_chat and calls onSetUp', async () => {
    const onSetUp = vi.fn();
    const add = { id: 'fake', label: 'Fake' };
    await menu({ agents: [], onStart: vi.fn(async () => undefined), canAdd: [add], onSetUp });
    expect(document.querySelectorAll('.ck-menu-setup')).toHaveLength(1);
    expect(host.querySelector('[data-testid="setup-row-other"]')).toBeNull();
    await click(host.querySelector('[aria-label="Set up chat for Fake"]'));
    expect(onSetUp).toHaveBeenCalledWith(add);
  });

  it('shows no hint when nothing can be added (Labs off sends an empty list)', async () => {
    await menu({
      agents: [],
      onStart: vi.fn(async () => undefined),
      canAdd: addableChat({ labs_enabled: false, agents: [], can_add_chat: [{ id: 'fake', label: 'Fake' }] }),
      onSetUp: vi.fn(),
    });
    expect(host.querySelector('.ck-menu-setup')).toBeNull();
    await menu(undefined);
    expect(host.querySelector('.ck-menu-setup')).toBeNull();
  });
});
