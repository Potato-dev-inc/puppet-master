// The chat model (types, mergeItems, groupTurns) has no Tauri import, so the phone bundle can use it; the desktop
// module re-exports the same functions.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => {
  throw new Error('the model must not load Tauri');
});
vi.mock('@tauri-apps/api/event', () => {
  throw new Error('the model must not load Tauri');
});

import { groupTurns, mergeItems, type ChatItem } from './sessionChatModel';

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
