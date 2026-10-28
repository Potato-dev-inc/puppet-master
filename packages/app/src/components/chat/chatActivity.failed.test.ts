import { describe, expect, it } from 'vitest';
import type { ToolStatus, ToolStep } from '../../lib/sessionChatModel';
import { activitySummary, activitySummaryParts } from './chatActivity';

function tool(id: string, name: string, status: ToolStatus | undefined, extra: Partial<ToolStep['call']> = {}): ToolStep {
  const call = { kind: 'toolCall' as const, id, at: null, callId: id, name, summary: `${name} target`, ...extra };
  // A row read off the screen has no status at all, so the field can be missing.
  return { kind: 'tool', id, call, result: null, status: status as ToolStatus };
}

describe('activitySummary with failed steps', () => {
  it('adds the failed count to its group: 2 commands, one error', () => {
    expect(activitySummary([tool('1', 'Bash', 'ok'), tool('2', 'Bash', 'error')])).toBe('Ran 2 commands (1 failed)');
  });

  it('keeps a group with no failures as it was', () => {
    expect(activitySummary([tool('1', 'Read', 'ok', { paths: ['a.ts'] }), tool('2', 'Bash', 'error')])).toBe('Read 1 file · Ran 1 command (1 failed)');
  });

  it('counts failures per group, not across the whole line', () => {
    const steps = [tool('1', 'Read', 'error', { paths: ['a.ts'] }), tool('2', 'Bash', 'ok'), tool('3', 'Bash', 'error')];
    expect(activitySummary(steps)).toBe('Read 1 file (1 failed) · Ran 2 commands (1 failed)');
  });

  it('counts every failed row of a group, even when they touch one file', () => {
    const steps = [tool('1', 'Edit', 'error', { paths: ['a.ts'], writes: true }), tool('2', 'Edit', 'error', { paths: ['a.ts'], writes: true })];
    expect(activitySummary(steps)).toBe('Edited 1 file (2 failed)');
  });

  it('never adds the word failed when no row failed', () => {
    expect(activitySummary([tool('1', 'Bash', 'ok'), tool('2', 'Bash', 'running')])).toBe('Ran 2 commands');
  });

  it('gives screen-read rows with no status no count', () => {
    const steps = [tool('1', 'Bash', undefined), tool('2', 'Bash', undefined), tool('3', 'Read', undefined, { paths: ['a.ts'] })];
    const summary = activitySummary(steps);
    expect(summary).toBe('Ran 2 commands · Read 1 file');
    expect(summary).not.toContain('failed');
  });

  it('keeps the failed part in its own piece, for styling', () => {
    expect(activitySummaryParts([tool('1', 'Bash', 'ok'), tool('2', 'Bash', 'error'), tool('3', 'Read', 'ok', { paths: ['a.ts'] })])).toEqual([
      { text: 'Ran 2 commands', failed: '(1 failed)' },
      { text: 'Read 1 file', failed: null },
    ]);
  });
});
