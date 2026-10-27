// The chat model (types, mergeItems, groupTurns) has no Tauri import, so the phone bundle can use it; the desktop
// module re-exports the same functions.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => {
  throw new Error('the model must not load Tauri');
});
vi.mock('@tauri-apps/api/event', () => {
  throw new Error('the model must not load Tauri');
});

import { foldPlan, groupTurns, mergeItems, type ChatItem, type PlanChatItem, type PlanItem } from './sessionChatModel';

const user = (id: string, text: string): ChatItem => ({ kind: 'user', id, at: null, text });

describe('sessionChatModel without Tauri', () => {
  it('merges by id in place and replaces on reset', () => {
    const merged = mergeItems([user('1', 'a'), user('2', 'b')], [user('2', 'b!'), user('3', 'c')], false);
    expect(merged.map((item) => (item.kind === 'user' ? item.text : ''))).toEqual(['a', 'b!', 'c']);
    expect(mergeItems(merged, [user('9', 'z')], true).map((item) => item.id)).toEqual(['9']);
  });

  it('keeps only the newest usage item', () => {
    const usage = (id: string, input: number): ChatItem => ({ kind: 'usage', id, at: null, inputTokens: input, outputTokens: 0, contextWindow: null });
    const merged = mergeItems([usage('1', 10)], [usage('2', 20)], false);
    expect(merged).toEqual([usage('2', 20)]);
  });

  it('groups a user message, a tool and its result into one turn', () => {
    const grouped = groupTurns([
      user('1', 'run it'),
      { kind: 'toolCall', id: '2', at: null, callId: 'c', name: 'shell', summary: 'ls' },
      { kind: 'toolResult', id: '3', at: null, callId: 'c', ok: true, output: 'a' },
      { kind: 'turnEnd', id: '4', at: null, outcome: 'completed', durationMs: 1000 },
    ]);
    expect(grouped.turns).toHaveLength(1);
    const step = grouped.turns[0]!.steps[0]!;
    expect(step.kind === 'tool' && step.status).toBe('ok');
    expect(grouped.turns[0]!.outcome).toBe('completed');
  });
});

const call = (id: string, callId: string, name: string, extra: Record<string, unknown> = {}): ChatItem =>
  ({ kind: 'toolCall', id, at: null, callId, name, summary: '', ...extra }) as ChatItem;
const plan = (id: string, replace: boolean, items: PlanChatItem['items']): PlanChatItem => ({ kind: 'plan', id, at: null, items, replace });
const notice = (id: string, noticeKind: string, text: string, url?: string): ChatItem => ({ kind: 'notice', id, at: null, noticeKind, text, url });
const end = (id: string, durationMs: number | null, outcome: 'completed' | 'interrupted' = 'completed'): ChatItem => ({ kind: 'turnEnd', id, at: null, outcome, durationMs });

describe('foldPlan', () => {
  it('a full list replaces what was there and defaults a missing status to pending', () => {
    const before: PlanItem[] = [{ text: 'old', status: 'completed' }];
    const next = foldPlan(before, plan('1', true, [{ text: 'a', status: 'in_progress' }, { text: 'b' }]));
    expect(next).toEqual([{ text: 'a', status: 'in_progress' }, { text: 'b', status: 'pending' }]);
    expect(before).toEqual([{ text: 'old', status: 'completed' }]);
  });

  it('updates add, change, rename and delete tasks by taskId', () => {
    let list: PlanItem[] = [];
    list = foldPlan(list, plan('1', false, [{ text: 'Define', status: 'pending', taskId: '1' }]));
    list = foldPlan(list, plan('2', false, [{ text: 'Add', status: 'pending', taskId: '2' }]));
    list = foldPlan(list, plan('3', false, [{ text: '', status: 'completed', taskId: '1' }]));
    expect(list).toEqual([
      { text: 'Define', status: 'completed', taskId: '1' },
      { text: 'Add', status: 'pending', taskId: '2' },
    ]);
    list = foldPlan(list, plan('4', false, [{ text: 'Add it', taskId: '2' }]));
    expect(list[1]).toEqual({ text: 'Add it', status: 'pending', taskId: '2' });
    list = foldPlan(list, plan('5', false, [{ text: '', status: 'deleted', taskId: '1' }]));
    expect(list).toEqual([{ text: 'Add it', status: 'pending', taskId: '2' }]);
  });

  it('an update for a task it never saw is shown with a placeholder text, and one without an id is ignored', () => {
    expect(foldPlan([], plan('1', false, [{ text: '', status: 'in_progress', taskId: '7' }]))).toEqual([
      { text: 'Task 7', status: 'in_progress', taskId: '7' },
    ]);
    expect(foldPlan([], plan('2', false, [{ text: 'x', status: 'pending' }]))).toEqual([]);
  });
});

describe('groupTurns plan', () => {
  it('keeps the latest full plan per turn and the running plan for the session', () => {
    const grouped = groupTurns([
      user('1', 'start'),
      plan('2', true, [{ text: 'a', status: 'in_progress' }, { text: 'b', status: 'pending' }]),
      plan('3', true, [{ text: 'a', status: 'completed' }, { text: 'b', status: 'in_progress' }]),
      end('4', 1000),
      user('5', 'next'),
      end('6', 500),
      user('7', 'again'),
      plan('8', false, [{ text: '', status: 'completed', taskId: 'x' }]),
    ]);
    expect(grouped.turns[0]!.plan).toEqual([{ text: 'a', status: 'completed' }, { text: 'b', status: 'in_progress' }]);
    expect(grouped.turns[1]!.plan).toBeNull();
    // an update does not drop the tasks of the full list before it
    expect(grouped.turns[2]!.plan).toEqual([
      { text: 'a', status: 'completed' },
      { text: 'b', status: 'in_progress' },
      { text: 'Task x', status: 'completed', taskId: 'x' },
    ]);
    expect(grouped.plan).toEqual(grouped.turns[2]!.plan);
  });

  it('builds the list task by task across turns', () => {
    const grouped = groupTurns([
      user('1', 'plan'),
      plan('2', false, [{ text: 'One', status: 'pending', taskId: '1' }]),
      plan('3', false, [{ text: 'Two', status: 'pending', taskId: '2' }]),
      end('4', null),
      user('5', 'go'),
      plan('6', false, [{ text: '', status: 'completed', taskId: '1' }]),
    ]);
    expect(grouped.plan.map((task) => `${task.taskId}:${task.status}`)).toEqual(['1:completed', '2:pending']);
    expect(grouped.turns[0]!.plan).toHaveLength(2);
    expect(grouped.turns[1]!.plan).toHaveLength(2);
  });

  it('a chat with no plan has an empty list', () => {
    expect(groupTurns([user('1', 'hi')]).plan).toEqual([]);
  });
});

describe('groupTurns turn end', () => {
  it('a later turn end only fills in the length of the turn that ended', () => {
    const grouped = groupTurns([user('1', 'hi'), end('2', null), end('3', 7000)]);
    expect(grouped.turns).toHaveLength(1);
    expect(grouped.turns[0]!.durationMs).toBe(7000);
    expect(grouped.turns[0]!.end!.id).toBe('2');
  });

  it('a late turn end with no length changes nothing, and an interrupted end keeps its outcome', () => {
    const grouped = groupTurns([user('1', 'hi'), end('2', null, 'interrupted'), end('3', null), user('4', 'again'), end('5', 100)]);
    expect(grouped.turns).toHaveLength(2);
    expect(grouped.turns[0]!.outcome).toBe('interrupted');
    expect(grouped.turns[0]!.durationMs).toBeNull();
    expect(grouped.turns[1]!.durationMs).toBe(100);
  });
});

describe('groupTurns notices', () => {
  it('shows a notice as a step of the turn it came in, or of the turn before when it came after the end', () => {
    const grouped = groupTurns([
      user('1', 'hi'),
      notice('2', 'compacted', 'Context compacted'),
      end('3', 10),
      notice('4', 'error', 'API error 529'),
    ]);
    expect(grouped.turns).toHaveLength(1);
    expect(grouped.turns[0]!.steps.map((step) => step.kind)).toEqual(['notice', 'notice']);
  });

  it('takes the newest title out of the steps and shows a pull request link once', () => {
    const grouped = groupTurns([
      notice('1', 'title', 'First'),
      user('2', 'hi'),
      notice('3', 'pr', 'Pull request #1', 'https://example.test/1'),
      notice('4', 'title', 'Second'),
      notice('5', 'pr', 'Pull request #1', 'https://example.test/1'),
      notice('6', 'pr', 'Pull request #2', 'https://example.test/2'),
    ]);
    expect(grouped.title).toBe('Second');
    expect(grouped.turns).toHaveLength(1);
    expect(grouped.turns[0]!.steps).toHaveLength(2);
  });

  it('a chat with a title and no messages has no empty turn', () => {
    const grouped = groupTurns([notice('1', 'title', 'Only a title')]);
    expect(grouped.turns).toEqual([]);
    expect(grouped.title).toBe('Only a title');
  });
});

describe('groupTurns outputs', () => {
  it('lists each path once in order of first touch with the last tool and whether anything wrote it', () => {
    const grouped = groupTurns([
      user('1', 'edit'),
      call('2', 'c1', 'Read', { paths: ['a.rs'] }),
      call('3', 'c2', 'Write', { paths: ['b.rs'], writes: true }),
      call('4', 'c3', 'Edit', { paths: ['a.rs'], writes: true }),
      call('5', 'c4', 'Read', { paths: ['b.rs'] }),
      call('6', 'c5', 'Bash'),
    ]);
    expect(grouped.outputs).toEqual([
      { path: 'a.rs', tool: 'Edit', wrote: true },
      { path: 'b.rs', tool: 'Read', wrote: true },
    ]);
  });

  it('a path only read is not marked as written, and a call seen twice counts once', () => {
    const grouped = groupTurns([call('1', 'c1', 'Read', { paths: ['a.rs'] }), call('2', 'c1', 'Read', { paths: ['a.rs'] })]);
    expect(grouped.outputs).toEqual([{ path: 'a.rs', tool: 'Read', wrote: false }]);
  });

  it('a call from an older backend has no paths', () => {
    expect(groupTurns([call('1', 'c1', 'shell')]).outputs).toEqual([]);
  });
});

describe('unknown kinds', () => {
  it('are ignored by groupTurns and kept by mergeItems', () => {
    const odd = { kind: 'somethingNew', id: 'x', at: null } as unknown as ChatItem;
    const grouped = groupTurns([user('1', 'hi'), odd, null as unknown as ChatItem, end('2', 5)]);
    expect(grouped.turns).toHaveLength(1);
    expect(grouped.turns[0]!.steps).toEqual([]);
    expect(mergeItems([], [odd], false)).toEqual([odd]);
  });
});
