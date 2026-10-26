import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn(async (_event: string, _cb: (e: { payload: unknown }) => void) => () => {});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...(args as Parameters<typeof listen>)) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  closeSessionChat,
  groupTurns,
  mergeItems,
  onSessionChatItems,
  openSessionChat,
  type ChatItem,
} from './sessionChat';

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

const user = (id: string, text = 'hi'): ChatItem => ({ kind: 'user', id, at: null, text });
const say = (id: string, text: string): ChatItem => ({
  kind: 'assistant',
  id,
  at: null,
  text,
  model: null,
  finalAnswer: false,
});
const call = (id: string, callId: string, name = 'Bash', summary = 'ls'): ChatItem => ({
  kind: 'toolCall',
  id,
  at: null,
  callId,
  name,
  summary,
});
const result = (id: string, callId: string, ok = true, output = 'out'): ChatItem => ({
  kind: 'toolResult',
  id,
  at: null,
  callId,
  ok,
  output,
});
const end = (id: string, outcome: 'completed' | 'interrupted' | 'failed' = 'completed', durationMs: number | null = null): ChatItem => ({
  kind: 'turnEnd',
  id,
  at: null,
  outcome,
  durationMs,
});

describe('mergeItems', () => {
  it('keeps order and appends new items', () => {
    const merged = mergeItems([user('1:0')], [say('2:0', 'a'), say('3:0', 'b')], false);
    expect(merged.map((i) => i.id)).toEqual(['1:0', '2:0', '3:0']);
  });

  it('dedupes by id, keeping the first position and the newest copy', () => {
    const prev = [say('1:0', 'old'), say('2:0', 'x')];
    const merged = mergeItems(prev, [say('1:0', 'new')], false);
    expect(merged).toHaveLength(2);
    expect(merged[0]).toEqual(say('1:0', 'new'));
    expect(merged[1]).toEqual(say('2:0', 'x'));
  });

  it('reset drops what was there before', () => {
    const merged = mergeItems([user('1:0'), say('2:0', 'a')], [user('9:0', 'fresh')], true);
    expect(merged).toEqual([user('9:0', 'fresh')]);
  });

  it('does not change its inputs', () => {
    const prev = [user('1:0')];
    const incoming = [say('2:0', 'a')];
    mergeItems(prev, incoming, false);
    expect(prev).toHaveLength(1);
    expect(incoming).toHaveLength(1);
  });
});

describe('groupTurns', () => {
  it('starts a turn at a user item and ends it at turnEnd', () => {
    const { turns } = groupTurns([
      user('1:0', 'first'),
      say('2:0', 'answer'),
      end('3:0'),
      user('4:0', 'second'),
      say('5:0', 'more'),
    ]);
    expect(turns).toHaveLength(2);
    expect(turns[0].user?.text).toBe('first');
    expect(turns[0].steps.map((s) => s.id)).toEqual(['2:0']);
    expect(turns[0].outcome).toBe('completed');
    expect(turns[1].user?.text).toBe('second');
    expect(turns[1].end).toBeNull();
  });

  it('attaches a tool result to its call by callId', () => {
    const { turns } = groupTurns([user('1:0'), call('2:0', 'c1'), result('3:0', 'c1', true, 'files'), end('4:0')]);
    const step = turns[0].steps[0];
    expect(step.kind).toBe('tool');
    if (step.kind !== 'tool') throw new Error('expected a tool step');
    expect(step.result?.output).toBe('files');
    expect(step.status).toBe('ok');
  });

  it('marks an error result as error', () => {
    const { turns } = groupTurns([user('1:0'), call('2:0', 'c1'), result('3:0', 'c1', false), end('4:0')]);
    const step = turns[0].steps[0];
    if (step.kind !== 'tool') throw new Error('expected a tool step');
    expect(step.status).toBe('error');
  });

  it('shows a call with no result yet as running', () => {
    const { turns } = groupTurns([user('1:0'), call('2:0', 'c1', 'Edit', 'src/a.ts')]);
    const step = turns[0].steps[0];
    if (step.kind !== 'tool') throw new Error('expected a tool step');
    expect(step.result).toBeNull();
    expect(step.status).toBe('running');
    expect(turns[0].end).toBeNull();
  });

  it('keeps an interrupted turn interrupted, with its duration', () => {
    const { turns } = groupTurns([user('1:0'), say('2:0', 'working'), end('3:0', 'interrupted', 12_000)]);
    expect(turns[0].outcome).toBe('interrupted');
    expect(turns[0].durationMs).toBe(12_000);
  });

  it('keeps agent output that comes before any user message in a turn without a user', () => {
    const { turns } = groupTurns([say('1:0', 'resumed'), end('2:0', 'failed')]);
    expect(turns).toHaveLength(1);
    expect(turns[0].user).toBeNull();
    expect(turns[0].outcome).toBe('failed');
  });

  it('takes the latest session for model and version, and the latest usage', () => {
    const grouped = groupTurns([
      { kind: 'session', id: '1:0', at: null, model: 'old-model', cliVersion: '1.0.0' },
      user('2:0'),
      { kind: 'usage', id: '3:0', at: null, inputTokens: 10, outputTokens: 2, contextWindow: 200 },
      { kind: 'session', id: '4:0', at: null, model: 'new-model', cliVersion: '1.1.0' },
      { kind: 'usage', id: '5:0', at: null, inputTokens: 30, outputTokens: 5, contextWindow: 200 },
    ]);
    expect(grouped.model).toBe('new-model');
    expect(grouped.cliVersion).toBe('1.1.0');
    expect(grouped.usage?.inputTokens).toBe(30);
    expect(grouped.turns).toHaveLength(1);
  });

  it('returns nothing for no items', () => {
    expect(groupTurns([])).toEqual({ turns: [], model: null, cliVersion: null, usage: null });
  });
});

describe('Tauri wrappers', () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockClear();
  });
  afterEach(() => setTauri(false));

  it('open and close call the backend with the pane id', async () => {
    setTauri(true);
    invoke.mockResolvedValueOnce({ available: false, reason: 'no file yet', items: [] });
    await expect(openSessionChat('p1')).resolves.toEqual({ available: false, reason: 'no file yet', items: [] });
    expect(invoke).toHaveBeenLastCalledWith('session_chat_open', { paneId: 'p1' });
    invoke.mockResolvedValueOnce(undefined);
    await closeSessionChat('p1');
    expect(invoke).toHaveBeenLastCalledWith('session_chat_close', { paneId: 'p1' });
  });

  it('refuses outside the desktop shell', async () => {
    setTauri(false);
    await expect(openSessionChat('p1')).rejects.toThrow('Desktop shell required');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('subscribes to session-chat-items and unwraps the payload', async () => {
    setTauri(true);
    const cb = vi.fn();
    await onSessionChatItems(cb);
    expect(listen).toHaveBeenCalledWith('session-chat-items', expect.any(Function));
    const handler = listen.mock.calls[0][1] as unknown as (e: { payload: unknown }) => void;
    const payload = { paneId: 'p1', items: [user('1:0')], reset: false };
    handler({ payload });
    expect(cb).toHaveBeenCalledWith(payload);
  });
});
