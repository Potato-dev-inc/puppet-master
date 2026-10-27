// @vitest-environment jsdom
// The phone's chat view: it asks the PC every 1.5 s while shown and visible, asks only for what changed since the
// last answer, drops its list on a reset, stops when the page is hidden, and never asks without the watch cap.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../../lib/bridge-types';
import type { ChatItem, PhoneChatReply } from '../../lib/sessionChatModel';
import { publishBridgeOutbox } from '../../lib/paneOutbox-bridge';
import type { OutboxMessage } from '../../lib/paneOutbox';
import { NO_SEND_NOTE, NO_WATCH_NOTE, PhoneChatView, POLL_MS } from './PhoneChatView';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let visibility: DocumentVisibilityState = 'visible';

beforeEach(() => {
  vi.useFakeTimers();
  visibility = 'visible';
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const user = (id: string, text: string): ChatItem => ({ kind: 'user', id, at: null, text });
const reply = (over: Partial<PhoneChatReply>): PhoneChatReply => ({
  available: true,
  reason: null,
  source: 'file',
  waitingFor: null,
  rev: 1,
  reset: false,
  items: [],
  ...over,
});

function bridgeWith(answers: Array<PhoneChatReply | Error>) {
  const paneChat = vi.fn(async (_paneId: string, _since?: number) => {
    const next = answers.length > 1 ? answers.shift()! : answers[0]!;
    if (next instanceof Error) throw next;
    return next;
  });
  return { bridge: { paneChat } as unknown as BridgeClient, paneChat };
}

async function mount(props: { bridge: BridgeClient | null; canWatch?: boolean; canType?: boolean; sendBlock?: string | null; dock?: React.ReactNode; paneId?: string }) {
  await act(async () => {
    root.render(
      <PhoneChatView
        bridge={props.bridge}
        paneId={props.paneId ?? 'p1'}
        canWatch={props.canWatch ?? true}
        canType={props.canType ?? true}
        sendBlock={props.sendBlock}
        dock={props.dock}
      />,
    );
  });
}

/** Moves the clock and lets the answers that were waiting land. */
const advance = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
const text = () => host.textContent ?? '';

describe('PhoneChatView polling', () => {
  it('asks at once with no revision, then every 1.5 s with the revision it was given', async () => {
    const { bridge, paneChat } = bridgeWith([
      reply({ rev: 10, items: [user('1:0', 'hello')] }),
      reply({ rev: 12, items: [user('2:0', 'again')] }),
      reply({ rev: 12 }),
    ]);
    await mount({ bridge });
    expect(paneChat).toHaveBeenCalledTimes(1);
    expect(paneChat).toHaveBeenLastCalledWith('p1', undefined);
    expect(text()).toContain('hello');

    await advance(POLL_MS);
    expect(paneChat).toHaveBeenCalledTimes(2);
    expect(paneChat).toHaveBeenLastCalledWith('p1', 10);
    expect(text()).toContain('hello');
    expect(text()).toContain('again');

    await advance(POLL_MS);
    expect(paneChat).toHaveBeenLastCalledWith('p1', 12);
  });

  it('merges a changed item in place by its id and keeps the rest', async () => {
    const { bridge } = bridgeWith([
      reply({ rev: 1, items: [user('1:0', 'first'), { kind: 'assistant', id: '2:0', at: null, text: 'par', model: null, finalAnswer: false }] }),
      reply({ rev: 2, items: [{ kind: 'assistant', id: '2:0', at: null, text: 'partial answer', model: null, finalAnswer: false }] }),
    ]);
    await mount({ bridge });
    await advance(POLL_MS);
    expect(text()).toContain('first');
    expect(text()).toContain('partial answer');
    expect(host.querySelectorAll('.ck-chat-text')).toHaveLength(1);
  });

  it('drops the list it holds on a reset', async () => {
    const { bridge } = bridgeWith([
      reply({ rev: 1, items: [user('1:0', 'old chat')] }),
      reply({ rev: 5, reset: true, items: [user('9:0', 'new chat')] }),
    ]);
    await mount({ bridge });
    expect(text()).toContain('old chat');
    await advance(POLL_MS);
    expect(text()).toContain('new chat');
    expect(text()).not.toContain('old chat');
  });

  it('stops asking while the page is hidden and asks again, from its revision, when it is back', async () => {
    const { bridge, paneChat } = bridgeWith([reply({ rev: 7, items: [user('1:0', 'hi')] })]);
    await mount({ bridge });
    expect(paneChat).toHaveBeenCalledTimes(1);

    visibility = 'hidden';
    await act(async () => { document.dispatchEvent(new Event('visibilitychange')); });
    await advance(POLL_MS * 5);
    expect(paneChat).toHaveBeenCalledTimes(1);

    visibility = 'visible';
    await act(async () => { document.dispatchEvent(new Event('visibilitychange')); });
    expect(paneChat).toHaveBeenCalledTimes(2);
    expect(paneChat).toHaveBeenLastCalledWith('p1', 7);
    expect(text()).toContain('hi');
  });

  it('stops asking when it is removed', async () => {
    const { bridge, paneChat } = bridgeWith([reply({ rev: 1 })]);
    await mount({ bridge });
    act(() => root.unmount());
    await advance(POLL_MS * 4);
    expect(paneChat).toHaveBeenCalledTimes(1);
    root = createRoot(host);
  });

  it('never has two questions in flight', async () => {
    let release: (value: PhoneChatReply) => void = () => undefined;
    const paneChat = vi.fn(() => new Promise<PhoneChatReply>((resolve) => { release = resolve; }));
    await mount({ bridge: { paneChat } as unknown as BridgeClient });
    await advance(POLL_MS * 4);
    expect(paneChat).toHaveBeenCalledTimes(1);
    await act(async () => { release(reply({ rev: 1 })); });
    await advance(POLL_MS);
    expect(paneChat).toHaveBeenCalledTimes(2);
  });
});

describe('PhoneChatView states', () => {
  it('a phone without watch asks nothing and says so in one plain line', async () => {
    const { bridge, paneChat } = bridgeWith([reply({})]);
    await mount({ bridge, canWatch: false, dock: <div data-testid="dock">card</div> });
    await advance(POLL_MS * 3);
    expect(paneChat).not.toHaveBeenCalled();
    expect(text()).toContain(NO_WATCH_NOTE);
    expect(NO_WATCH_NOTE).toBe('This phone cannot read chats.');
    expect(host.querySelector('[data-testid="dock"]')).not.toBeNull();
  });

  it('a refusal from the PC (403) shows the same line and stops asking', async () => {
    const { bridge, paneChat } = bridgeWith([new Error('bridge GET /panes/p1/chat -> 403: {"error":"not_allowed"}')]);
    await mount({ bridge });
    await advance(POLL_MS * 3);
    expect(paneChat).toHaveBeenCalledTimes(1);
    expect(text()).toContain(NO_WATCH_NOTE);
  });

  it('a pane that is not an agent shows the PC\'s reason', async () => {
    const { bridge } = bridgeWith([reply({ available: false, reason: 'Chat is for agent panes.', source: null })]);
    await mount({ bridge });
    expect(text()).toContain('Chat unavailable');
    expect(text()).toContain('Chat is for agent panes.');
  });

  it('an empty chat shows what the PC says it waits for', async () => {
    const { bridge } = bridgeWith([reply({ waitingFor: 'Nothing to show yet. Waiting for the agent.' })]);
    await mount({ bridge });
    expect(text()).toContain('Nothing to show yet. Waiting for the agent.');
  });

  it('a dropped link keeps the chat on screen and tries again', async () => {
    const { bridge, paneChat } = bridgeWith([
      reply({ rev: 3, items: [user('1:0', 'still here')] }),
      new Error('Failed to fetch'),
      reply({ rev: 4, items: [user('2:0', 'and more')] }),
    ]);
    await mount({ bridge });
    await advance(POLL_MS);
    expect(text()).toContain('still here');
    await advance(POLL_MS);
    expect(paneChat).toHaveBeenCalledTimes(3);
    expect(paneChat).toHaveBeenLastCalledWith('p1', 3);
    expect(text()).toContain('and more');
  });

  it('says so when the PC cannot be reached before anything was loaded', async () => {
    const { bridge } = bridgeWith([new Error('Failed to fetch')]);
    await mount({ bridge });
    expect(text()).toContain('Could not reach your PC');
  });

  it('a screen-read chat says so', async () => {
    const { bridge } = bridgeWith([reply({ source: 'screen', items: [user('screen:1:user', 'hi')] })]);
    await mount({ bridge });
    expect(host.querySelector('[data-testid="session-chat-screen-note"]')).not.toBeNull();
  });

  it('puts the docked card under the conversation', async () => {
    const { bridge } = bridgeWith([reply({ items: [user('1:0', 'hi')] })]);
    await mount({ bridge, dock: <div data-testid="dock">Allow?</div> });
    const dock = host.querySelector('.ck-chat-dock');
    expect(dock?.textContent).toBe('Allow?');
    expect(host.querySelector('[data-testid="phone-chat"]')).not.toBeNull();
  });
});

describe('PhoneChatView message box', () => {
  const queued = (over: Partial<OutboxMessage> = {}): OutboxMessage => ({
    id: 'm1',
    text: 'fix the login test',
    createdAt: 1,
    state: 'queued',
    waitingFor: 'The agent is working. Sends when the turn ends.',
    error: null,
    ...over,
  });

  function bridgeWithBox(over: Partial<BridgeClient> = {}) {
    const sendPaneMessage = vi.fn(async (_paneId: string, text: string) => queued({ id: 'm9', text }));
    const listPaneMessages = vi.fn(async () => [] as OutboxMessage[]);
    const cancelPaneMessage = vi.fn(async () => queued({ state: 'cancelled' }));
    const { paneChat } = bridgeWith([reply({ rev: 1 })]);
    const bridge = { paneChat, sendPaneMessage, listPaneMessages, cancelPaneMessage, ...over } as unknown as BridgeClient;
    return { bridge, sendPaneMessage, listPaneMessages, cancelPaneMessage };
  }

  const box = () => host.querySelector('[data-testid="chat-composer"]');
  const field = () => host.querySelector('textarea') as HTMLTextAreaElement;

  function typeAndSend(text: string) {
    const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
    act(() => {
      set.call(field(), text);
      field().dispatchEvent(new Event('input', { bubbles: true }));
    });
    act(() => {
      field().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    });
  }

  it('shows the message box when the phone may type and watch', async () => {
    const { bridge } = bridgeWithBox();
    await mount({ bridge });
    expect(box()).not.toBeNull();
    expect(host.querySelector('[data-testid="phone-chat-no-send"]')).toBeNull();
  });

  it('without the type cap it shows no box and one plain line', async () => {
    const { bridge, listPaneMessages } = bridgeWithBox();
    await mount({ bridge, canType: false });
    expect(box()).toBeNull();
    expect(host.querySelector('textarea')).toBeNull();
    expect(text()).toContain('This phone cannot send messages.');
    expect(NO_SEND_NOTE).toBe('This phone cannot send messages.');
    expect(listPaneMessages).not.toHaveBeenCalled();
  });

  it('without the watch cap it shows no box, and the chat note stays', async () => {
    const { bridge, listPaneMessages } = bridgeWithBox();
    await mount({ bridge, canWatch: false });
    expect(box()).toBeNull();
    expect(text()).toContain(NO_WATCH_NOTE);
    expect(listPaneMessages).not.toHaveBeenCalled();
  });

  it('sends the text through the bridge and shows it as a waiting bubble', async () => {
    const { bridge, sendPaneMessage } = bridgeWithBox();
    await mount({ bridge });
    typeAndSend('  fix the login test ');
    await act(async () => { await Promise.resolve(); });
    expect(sendPaneMessage).toHaveBeenCalledWith('p1', 'fix the login test');
    expect(field().value).toBe('');
    const bubble = host.querySelector('[data-testid="chat-pending"]');
    expect(bubble?.getAttribute('data-state')).toBe('queued');
    expect(bubble?.textContent).toContain('fix the login test');
    expect(host.querySelector('[data-testid="chat-pending-waiting"]')?.textContent).toBe('The agent is working. Sends when the turn ends.');
  });

  it('a queued bubble can be cancelled through the bridge', async () => {
    const { bridge, cancelPaneMessage } = bridgeWithBox({
      listPaneMessages: vi.fn(async () => [queued()]),
    } as Partial<BridgeClient>);
    await mount({ bridge });
    await act(async () => { await Promise.resolve(); });
    const cancel = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Cancel') as HTMLButtonElement;
    await act(async () => { cancel.click(); await Promise.resolve(); });
    expect(cancelPaneMessage).toHaveBeenCalledWith('p1', 'm1');
  });

  it('shows the messages the PC pushes with pane-outbox', async () => {
    const { bridge } = bridgeWithBox();
    await mount({ bridge });
    await act(async () => { publishBridgeOutbox('p1', [queued({ id: 'pushed', text: 'from the event' })]); });
    expect(host.querySelector('[data-testid="chat-pending"]')?.textContent).toContain('from the event');
  });

  it('a send block shows its sentence instead of a hint and switches the box off', async () => {
    const { bridge } = bridgeWithBox();
    await mount({ bridge, sendBlock: 'The agent is not running. Open Terminal.' });
    expect(text()).toContain('The agent is not running. Open Terminal.');
    expect(field().disabled).toBe(true);
  });
});
