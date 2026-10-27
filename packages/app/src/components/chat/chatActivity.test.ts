import { describe, expect, it } from 'vitest';
import type { ToolStep, TurnStep } from '../../lib/sessionChatModel';
import { activitySpanMs, activitySummary, countActivity, runningLabel, segmentSteps } from './chatActivity';

function tool(id: string, name: string, extra: Partial<ToolStep['call']> = {}, status: ToolStep['status'] = 'ok'): ToolStep {
  const call = { kind: 'toolCall' as const, id, at: null, callId: id, name, summary: `${name} target`, ...extra };
  return { kind: 'tool', id, call, result: null, status };
}

const text = (id: string): TurnStep => ({
  kind: 'text',
  id,
  item: { kind: 'assistant', id, at: null, text: 'hi', model: null, finalAnswer: false },
});

describe('segmentSteps', () => {
  it('folds consecutive tool rows into one activity segment', () => {
    const steps: TurnStep[] = [tool('1', 'Read'), tool('2', 'Bash'), tool('3', 'Edit')];
    const segments = segmentSteps(steps);
    expect(segments).toHaveLength(1);
    expect(segments[0]).toMatchObject({ kind: 'activity', id: '1' });
    expect(segments[0]!.kind === 'activity' && segments[0].steps.length).toBe(3);
  });

  it('breaks a run at a text step, and keeps helper rows out of the runs', () => {
    const steps: TurnStep[] = [tool('1', 'Read'), text('t'), tool('2', 'Read'), tool('h', 'Task', { helper: true }), tool('3', 'Bash')];
    const kinds = segmentSteps(steps).map((seg) => (seg.kind === 'activity' ? `activity:${seg.steps.length}` : seg.step.kind));
    expect(kinds).toEqual(['activity:1', 'text', 'activity:1', 'tool', 'activity:1']);
  });
});

describe('activitySummary', () => {
  it('counts by kind in the order the kinds first came', () => {
    const steps = [
      tool('1', 'Read', { paths: ['a.ts'] }),
      tool('2', 'Bash'),
      tool('3', 'Read', { paths: ['b.ts'] }),
      tool('4', 'Bash'),
      tool('5', 'Edit', { paths: ['c.ts'], writes: true }),
    ];
    expect(activitySummary(steps)).toBe('Read 2 files · Ran 2 commands · Edited 1 file');
  });

  it('counts a file once however often it was read or edited', () => {
    const steps = [tool('1', 'Read', { paths: ['a.ts'] }), tool('2', 'Read', { paths: ['a.ts'] }), tool('3', 'Read', { paths: ['b.ts', 'a.ts'] })];
    expect(countActivity(steps)).toEqual([{ kind: 'read', count: 2 }]);
  });

  it('says Used N tools for a tool it does not know', () => {
    expect(activitySummary([tool('1', 'TodoWrite'), tool('2', 'Mystery')])).toBe('Used 2 tools');
    expect(activitySummary([tool('1', 'TodoWrite')])).toBe('Used 1 tool');
  });

  it('uses the plural for searches, web uses and helpers', () => {
    expect(activitySummary([tool('1', 'Grep'), tool('2', 'Glob')])).toBe('Searched 2 times');
    expect(activitySummary([tool('1', 'WebFetch')])).toBe('Used the web 1 time');
  });
});

describe('runningLabel and activitySpanMs', () => {
  it('names the newest running row', () => {
    const steps = [tool('1', 'Bash', {}, 'ok'), tool('2', 'Bash', { summary: 'npm test' }, 'running')];
    expect(runningLabel(steps)).toBe('Bash npm test');
    expect(runningLabel([tool('1', 'Bash')])).toBeNull();
  });

  it('spans from the first start to the last end, or is null without times', () => {
    const first = tool('1', 'Read', { at: '2026-10-28T01:00:00Z' });
    const last = { ...tool('2', 'Edit', { at: '2026-10-28T01:00:20Z' }), result: { kind: 'toolResult' as const, id: 'r', at: '2026-10-28T01:00:48Z', callId: '2', ok: true, output: '' } };
    expect(activitySpanMs([first, last])).toBe(48_000);
    expect(activitySpanMs([tool('1', 'Read')])).toBeNull();
  });
});
