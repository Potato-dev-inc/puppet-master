// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { checkpointTitle, lastTurnStart } from './history-model';

describe('checkpointTitle', () => {
  it('names who asked instead of "a prompt"', () => {
    const row = { kind: 'answer', turn: 3, label: 'Before answering a prompt (turn 3)' };
    expect(checkpointTitle(row, 'Claude Code')).toBe("Before Claude Code's question (turn 3)");
    expect(checkpointTitle(row)).toBe("Before the agent's question (turn 3)");
  });

  it('keeps the backend label for every other kind', () => {
    expect(checkpointTitle({ kind: 'before', turn: 3, label: 'Before turn 3' }, 'Claude Code')).toBe('Before turn 3');
    expect(checkpointTitle({ label: '' })).toBe('Checkpoint');
  });
});

describe('lastTurnStart', () => {
  it('finds the start of the newest turn', () => {
    const items = [
      { id: 'a', turn: 3, kind: 'answer' },
      { id: 'b', turn: 3, kind: 'before' },
      { id: 'c', turn: 2, kind: 'after' },
    ];
    expect(lastTurnStart(items)?.id).toBe('b');
  });
});
