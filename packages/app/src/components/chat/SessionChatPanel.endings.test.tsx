// @vitest-environment jsdom
// How a turn of a chat ends, in the real chat view: one grey line under the turn, and the button that fits it.
//   Done · 3 files · 2 min                      (no button)
//   You stopped it                              (no button)
//   <Agent> hit its usage limit · Keep going with <other>   (only when another agent is set up)
//   Probably signed out · Sign in               (opens the "Check my setup" row)
//   The turn ended with no answer · Retry       (sends the message again)
//   Something went wrong: <cause> · Retry
// A message waiting for the turn to end shows "Send now" (stops the turn) and "Cancel".
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => undefined }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import type { ChatItem } from '../../lib/sessionChatModel';
import type { KeepGoingOffer } from '../../lib/keep-going';
import type { OutboxMessage } from '../../lib/paneOutbox';
import { registerSettingsOpener } from '../../lib/openSettingsAt';
import { SessionChatPanel } from './SessionChatPanel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let items: ChatItem[];
let offers: KeepGoingOffer[] | 'reject';
let queued: OutboxMessage[];

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const button = (text: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.startsWith(text));
const lines = () => [...host.querySelectorAll('[data-testid="turn-ending"]')];

/** A finished turn whose last words are `kind`/`text`, the way the backend sends it. */
function turn(n: number, kind: string, text: string): ChatItem[] {
  return [
    { kind: 'user', id: `u${n}`, at: null, text: `please do ${n}` },
    { kind: 'assistant', id: `a${n}.1`, at: null, text: 'ok', model: null, finalAnswer: false },
    { kind: 'notice', id: `n:ending:x:${n}`, at: null, noticeKind: `ending:${kind}`, text },
    { kind: 'turnEnd', id: `end:${n}`, at: null, outcome: kind === 'done' ? 'completed' : 'failed', durationMs: 1000 },
  ] as ChatItem[];
}

const offer = (label: string, ok = true): KeepGoingOffer => ({ target: { agent_id: label.toLowerCase().replace(/ /g, '-') }, label, ok, whyNot: ok ? null : 'not set up' });

function defaultInvoke() {
  invoke.mockImplementation(async (cmd: string, args: Record<string, unknown>) => {
    if (cmd === 'session_chat_open') return { available: true, reason: null, items, source: 'acp' };
    if (cmd === 'pane_messages_list') return queued;
    if (cmd === 'keep_going_targets') {
      if (offers === 'reject') throw new Error('NOT_SUPPORTED: this pane cannot hand over');
      return offers;
    }
    if (cmd === 'keep_going_lineage') return { from: null, to: null };
    if (cmd === 'keep_going_continue') return { id: 'kg-1' };
    if (cmd === 'pane_message_send') return { id: 'm9', text: String(args.text), createdAt: 1, state: 'queued', waitingFor: null, error: null };
    if (cmd === 'pane_message_cancel') return { ...queued[0], state: 'cancelled' };
    return undefined;
  });
}

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  items = [];
  offers = [];
  queued = [];
  defaultInvoke();
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  registerSettingsOpener(null);
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

async function mount(reply: { blockedReason: string | null; onInterrupt?: () => void } | null = { blockedReason: null }) {
  await act(async () => {
    root.render(<SessionChatPanel paneId="p1" reply={reply ?? undefined} />);
  });
  await flush();
  await flush();
}

describe('the line a turn ends with, in the chat view', () => {
  it('Done says the files and the time, and has no button', async () => {
    items = turn(1, 'done', 'Done · 3 files · 2 min');
    await mount();
    expect(lines().map((l) => l.textContent)).toEqual(['Done · 3 files · 2 min']);
    expect(host.querySelector('[data-ending-action]')).toBeNull();
    // It replaces "Worked for ..." instead of sitting next to it.
    expect(host.textContent).not.toContain('Worked for');
  });

  it('You stopped it has no button', async () => {
    items = turn(1, 'stopped', 'You stopped it');
    await mount();
    expect(lines()[0].textContent).toBe('You stopped it');
    expect(host.querySelector('[data-ending-action]')).toBeNull();
  });

  it('a usage limit offers Keep going with the other agent that is set up, and the button hands the task over', async () => {
    items = turn(1, 'usage_limit', 'Fake A hit its usage limit');
    offers = [offer('Fake B', false), offer('Fake C')];
    await mount();
    expect(lines()[0].textContent).toBe('Fake A hit its usage limit · Keep going with Fake C');
    await act(async () => button('Keep going with Fake C')!.click());
    await flush();
    expect(invoke).toHaveBeenCalledWith('keep_going_continue', { paneId: 'p1', target: 'fake-c', back: false, cardId: null });
  });

  it('a usage limit with no other agent set up is just the line', async () => {
    items = turn(1, 'usage_limit', 'Fake A hit its usage limit');
    offers = [offer('Fake B', false)];
    await mount();
    expect(lines()[0].textContent).toBe('Fake A hit its usage limit');
    offers = 'reject';
    act(() => root.unmount());
    root = createRoot(host);
    await mount();
    expect(lines()[0].textContent).toBe('Fake A hit its usage limit');
  });

  it('a hand-over that fails says why next to the line', async () => {
    items = turn(1, 'usage_limit', 'Fake A hit its usage limit');
    offers = [offer('Fake C')];
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation(async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === 'keep_going_continue') throw 'KEEP_GOING_FAILED: Fake C could not start';
      return base(cmd, args);
    });
    await mount();
    await act(async () => button('Keep going with Fake C')!.click());
    await flush();
    expect(host.querySelector('[data-testid="ending-note"]')?.textContent).toContain('Fake C could not start');
  });

  it('Probably signed out has Sign in, which opens the Check my setup row', async () => {
    items = turn(1, 'signed_out', 'Probably signed out');
    const open = vi.fn();
    registerSettingsOpener(open);
    await mount();
    expect(lines()[0].textContent).toBe('Probably signed out · Sign in');
    await act(async () => button('Sign in')!.click());
    expect(open).toHaveBeenCalledWith('setup-check');
  });

  it('a turn with no answer has Retry, which sends the same message again', async () => {
    items = turn(1, 'empty', 'The turn ended with no answer');
    await mount();
    expect(lines()[0].textContent).toBe('The turn ended with no answer · Retry');
    await act(async () => button('Retry')!.click());
    await flush();
    expect(invoke).toHaveBeenCalledWith('pane_message_send', { paneId: 'p1', text: 'please do 1' });
  });

  it('Something went wrong has Retry too', async () => {
    items = turn(1, 'failed', 'Something went wrong: the pipe broke');
    await mount();
    expect(lines()[0].textContent).toBe('Something went wrong: the pipe broke · Retry');
    await act(async () => button('Retry')!.click());
    expect(invoke).toHaveBeenCalledWith('pane_message_send', { paneId: 'p1', text: 'please do 1' });
  });

  it('a Retry that the backend refuses shows the reason', async () => {
    items = turn(1, 'failed', 'Something went wrong: the pipe broke');
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation(async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === 'pane_message_send') throw 'The pane is not open any more.';
      return base(cmd, args);
    });
    await mount();
    await act(async () => button('Retry')!.click());
    await flush();
    expect(host.querySelector('[data-testid="ending-note"]')?.textContent).toContain('The pane is not open any more.');
  });

  it('only the newest turn has buttons; older lines are plain words', async () => {
    items = [...turn(1, 'failed', 'Something went wrong: first'), ...turn(2, 'failed', 'Something went wrong: second')];
    await mount();
    expect(lines().map((l) => l.textContent)).toEqual(['Something went wrong: first', 'Something went wrong: second · Retry']);
  });

  it('a chat with no message box has no Retry', async () => {
    items = turn(1, 'failed', 'Something went wrong: the pipe broke');
    await mount(null);
    expect(lines()[0].textContent).toBe('Something went wrong: the pipe broke');
  });

  it('a chat whose turn did not come with a line keeps "Worked for"', async () => {
    items = [
      { kind: 'user', id: 'u1', at: null, text: 'hi' },
      { kind: 'turnEnd', id: 'end:1', at: null, outcome: 'completed', durationMs: 5000 },
    ] as ChatItem[];
    await mount();
    expect(host.textContent).toContain('Worked for');
    expect(lines()).toHaveLength(0);
  });
});

describe('a message waiting for the turn to end', () => {
  const waiting = (): OutboxMessage => ({ id: 'm1', text: 'next thing', createdAt: 1, state: 'queued', waitingFor: 'The agent is working. Sends when the turn ends.', error: null });

  it('says so, and Send now stops the turn while Cancel drops the message', async () => {
    queued = [waiting()];
    const interrupt = vi.fn();
    await mount({ blockedReason: null, onInterrupt: interrupt });
    const row = host.querySelector('[data-testid="chat-pending"]')!;
    expect(row.textContent).toContain('Waiting for the turn to end');
    expect(row.textContent).toContain('Send now');
    expect(row.textContent).toContain('Cancel');
    await act(async () => button('Send now')!.click());
    expect(interrupt).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalledWith('pane_message_cancel', expect.anything());
    await act(async () => button('Cancel')!.click());
    expect(invoke).toHaveBeenCalledWith('pane_message_cancel', { paneId: 'p1', messageId: 'm1' });
  });

  it('without a way to stop the turn it keeps the backend sentence and Cancel only', async () => {
    queued = [waiting()];
    await mount({ blockedReason: null });
    const row = host.querySelector('[data-testid="chat-pending"]')!;
    expect(row.textContent).toContain('The agent is working. Sends when the turn ends.');
    expect(row.textContent).not.toContain('Send now');
  });

  it('a message waiting for something else (an answer to give first) has no Send now', async () => {
    queued = [{ ...waiting(), waitingFor: 'The agent asks something. Answer it first.' }];
    await mount({ blockedReason: null, onInterrupt: vi.fn() });
    expect(host.querySelector('[data-testid="chat-pending"]')!.textContent).not.toContain('Send now');
  });
});
