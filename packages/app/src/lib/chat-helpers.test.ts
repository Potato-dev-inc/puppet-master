import { describe, expect, it } from 'vitest';
import { chatHelpers, helperStatus, placeHelpers, type ChatHelper } from './chat-helpers';
import { groupTurns, type ChatItem } from './sessionChat';

const pane = (id: string, status: string, title: string | null, parent?: string, createdAt = 1) => ({
  info: { id, title, created_at: createdAt, parent_pane_id: parent },
  status,
});
const name = () => 'Codex';

describe('chatHelpers', () => {
  it('lists the panes whose live parent is the pane, oldest first, with their agent and a plain status', () => {
    const list = [
      pane('p', 'running', 'Lead'),
      pane('b', 'idle', 'Map backend', 'p', 300),
      pane('a', 'running', 'Map frontend', 'p', 100),
      pane('c', 'error', null, 'p', 200),
      pane('other', 'running', 'Elsewhere', 'q', 50),
      pane('q', 'running', 'Another lead'),
    ];
    expect(chatHelpers(list, 'p', name).map((h) => [h.id, h.title, h.agent, h.status])).toEqual([
      ['a', 'Map frontend', 'Codex', 'working'],
      ['c', 'Codex', null, 'failed'],
      ['b', 'Map backend', 'Codex', 'done'],
    ]);
  });

  it('leaves out a helper whose parent is gone and a pane that is nobody a helper', () => {
    expect(chatHelpers([pane('h', 'running', 'x', 'closed'), pane('solo', 'running', 'y')], 'closed', name)).toEqual([]);
  });

  it('says needs you for a waiting pane or one with a card open, and failed beats both', () => {
    expect(helperStatus('waiting_input', false)).toBe('needs-you');
    expect(helperStatus('running', true)).toBe('needs-you');
    expect(helperStatus('error', true)).toBe('failed');
    expect(helperStatus('running', false)).toBe('working');
    expect(helperStatus('idle', false)).toBe('done');
    const list = [pane('p', 'running', 'Lead'), pane('h', 'running', 'Ask', 'p')];
    expect(chatHelpers(list, 'p', name, new Set(['h']))[0].status).toBe('needs-you');
  });
});

const items: ChatItem[] = [
  { kind: 'user', id: '1', at: '2026-10-30T10:00:00.000Z', text: 'first' },
  { kind: 'turnEnd', id: '2', at: '2026-10-30T10:05:00.000Z', outcome: 'completed', durationMs: 300_000 },
  { kind: 'user', id: '3', at: '2026-10-30T11:00:00.000Z', text: 'second' },
  { kind: 'turnEnd', id: '4', at: '2026-10-30T11:05:00.000Z', outcome: 'completed', durationMs: 300_000 },
  { kind: 'user', id: '5', at: '2026-10-30T12:00:00.000Z', text: 'third' },
];
const helper = (id: string, iso: string): ChatHelper => ({ id, title: id, agent: null, status: 'working', startedAt: Date.parse(iso) });

describe('placeHelpers', () => {
  const turns = groupTurns(items).turns;

  it('puts a helper in the turn during which it was started', () => {
    const placed = placeHelpers(turns, [helper('a', '2026-10-30T10:02:00Z'), helper('b', '2026-10-30T11:01:00Z'), helper('c', '2026-10-30T12:30:00Z'), helper('d', '2026-10-30T11:02:00Z')]);
    const byTurn = turns.map((t) => (placed.get(t.id) ?? []).map((h) => h.id));
    expect(byTurn).toEqual([['a'], ['b', 'd'], ['c']]);
  });

  it('puts a helper that predates every turn in the first one', () => {
    const placed = placeHelpers(turns, [helper('early', '2026-10-30T09:00:00Z')]);
    expect(placed.get(turns[0].id)?.map((h) => h.id)).toEqual(['early']);
  });

  it('puts every helper in the last (running) turn when no turn has a time', () => {
    const untimed = groupTurns([
      { kind: 'user', id: '1', at: null, text: 'a' },
      { kind: 'turnEnd', id: '2', at: null, outcome: 'completed', durationMs: null },
      { kind: 'user', id: '3', at: null, text: 'b' },
    ]).turns;
    const placed = placeHelpers(untimed, [helper('a', '2026-10-30T10:02:00Z')]);
    expect(placed.get(untimed[1].id)?.map((h) => h.id)).toEqual(['a']);
    expect(placed.has(untimed[0].id)).toBe(false);
  });

  it('places nothing when there are no turns', () => {
    expect(placeHelpers([], [helper('a', '2026-10-30T10:02:00Z')]).size).toBe(0);
  });
});
