// @vitest-environment jsdom
// The Chat tab with its message box: a message goes to the backend, shows as a bubble while it waits, can be
// cancelled, and the bubble goes when the conversation shows the real message.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const handlers = new Map<string, (e: { payload: unknown }) => void>();
const listen = vi.fn(async (event: string, cb: (e: { payload: unknown }) => void) => {
  handlers.set(event, cb);
  return () => handlers.delete(event);
});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => listen(...(a as Parameters<typeof listen>)) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import type { OutboxMessage } from '../../lib/paneOutbox';
import { SessionChatPanel } from './SessionChatPanel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const emit = (event: string, payload: unknown) => act(() => handlers.get(event)?.({ payload }));

const WAIT = 'The agent is working. Sends when the turn ends.';

function message(over: Partial<OutboxMessage> = {}): OutboxMessage {
  return { id: 'm1', text: 'hello agent', createdAt: 1, state: 'queued', waitingFor: null, error: null, ...over };
}

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  listen.mockClear();
  handlers.clear();
  invoke.mockImplementation(async (cmd: string, args: Record<string, unknown>) => {
    if (cmd === 'session_chat_open') return { available: true, reason: null, items: [], source: 'file' };
    if (cmd === 'pane_messages_list') return [];
    if (cmd === 'pane_message_send') return message({ text: String(args.text) });
    if (cmd === 'pane_message_cancel') return message({ state: 'cancelled' });
    return undefined;
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

function type(text: string) {
  const field = host.querySelector('textarea') as HTMLTextAreaElement;
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field, text);
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
  act(() => {
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
  });
}

const pending = () => [...host.querySelectorAll('[data-testid="chat-pending"]')];

async function open(reply: { blockedReason: string | null } | false = { blockedReason: null }) {
  act(() => root.render(<SessionChatPanel paneId="p1" reply={reply || undefined} />));
  await flush();
}

describe('SessionChatPanel with a message box', () => {
  it('sends the text, shows it as a waiting bubble, and follows the backend events', async () => {
    await open();
    type('hello agent');
    await flush();
    expect(invoke).toHaveBeenCalledWith('pane_message_send', { paneId: 'p1', text: 'hello agent' });
    expect(pending()).toHaveLength(1);
    expect(pending()[0].textContent).toContain('hello agent');
    expect((host.querySelector('textarea') as HTMLTextAreaElement).value).toBe('');

    emit('pane-outbox', { paneId: 'p1', messages: [message({ waitingFor: WAIT })] });
    expect(host.querySelector('[data-testid="chat-pending-waiting"]')?.textContent).toBe(WAIT);

    // Another pane's list is not ours.
    emit('pane-outbox', { paneId: 'other', messages: [] });
    expect(pending()).toHaveLength(1);

    emit('pane-outbox', { paneId: 'p1', messages: [message({ state: 'sending' })] });
    expect(pending()[0].getAttribute('data-state')).toBe('sending');
    expect([...host.querySelectorAll('button')].some((b) => b.textContent === 'Cancel')).toBe(false);
  });

  it('cancels a waiting message from the bubble', async () => {
    await open();
    emit('pane-outbox', { paneId: 'p1', messages: [message({ waitingFor: WAIT })] });
    const cancel = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')!;
    act(() => cancel.click());
    await flush();
    expect(invoke).toHaveBeenCalledWith('pane_message_cancel', { paneId: 'p1', messageId: 'm1' });
    emit('pane-outbox', { paneId: 'p1', messages: [message({ state: 'cancelled' })] });
    expect(pending()).toHaveLength(0);
  });

  it('drops the bubble when the conversation shows the real message', async () => {
    await open();
    emit('pane-outbox', { paneId: 'p1', messages: [message({ state: 'sent' })] });
    expect(pending()).toHaveLength(1);
    emit('session-chat-items', {
      paneId: 'p1',
      reset: false,
      items: [{ kind: 'user', id: 'u1', at: null, text: 'hello agent' }],
    });
    expect(pending()).toHaveLength(0);
    expect(host.textContent).toContain('hello agent');
  });

  it('shows a failed message with its error', async () => {
    await open();
    emit('pane-outbox', { paneId: 'p1', messages: [message({ state: 'failed', error: 'The agent is not running in this pane.' })] });
    expect(host.querySelector('[data-testid="chat-pending-error"]')?.textContent).toBe('The agent is not running in this pane.');
  });

  it('says why when the backend refuses the message, and keeps the text', async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'session_chat_open') return { available: true, reason: null, items: [], source: 'file' };
      if (cmd === 'pane_messages_list') return [];
      if (cmd === 'pane_message_send') throw 'An agent run controls this pane. Use Terminal, or wait until the run ends.';
      return undefined;
    });
    await open();
    type('hello agent');
    await flush();
    expect(host.querySelector('[data-testid="chat-composer-note"]')?.textContent).toContain('An agent run controls this pane.');
    expect((host.querySelector('textarea') as HTMLTextAreaElement).value).toBe('hello agent');
    expect(pending()).toHaveLength(0);
  });

  it('switches the box off with the reason', async () => {
    await open({ blockedReason: 'The agent is not running. Open Terminal.' });
    expect((host.querySelector('textarea') as HTMLTextAreaElement).disabled).toBe(true);
    expect(host.querySelector('[data-testid="chat-composer-note"]')?.textContent).toBe('The agent is not running. Open Terminal.');
  });

  it('has no message box and asks the backend for no messages without the reply option', async () => {
    await open(false);
    expect(host.querySelector('textarea')).toBeNull();
    expect(invoke.mock.calls.some(([cmd]) => String(cmd).startsWith('pane_message'))).toBe(false);
  });

  it('an "Edit in Chat" draft from the Point box goes into this pane box', async () => {
    localStorage.clear(); // no unsent text kept from an earlier test
    const before = invoke.getMockImplementation()!;
    invoke.mockImplementation(async (cmd: string, args: Record<string, unknown>) =>
      cmd === 'point_take_draft' ? 'Change the heading' : before(cmd, args),
    );
    await open();
    expect(invoke).toHaveBeenCalledWith('point_take_draft', { paneId: 'p1' });
    expect((host.querySelector('textarea') as HTMLTextAreaElement).value).toBe('Change the heading');
  });

  it('no Point draft is taken when the panel has no message box', async () => {
    await open(false);
    expect(invoke.mock.calls.some(([cmd]) => cmd === 'point_take_draft')).toBe(false);
  });
});
