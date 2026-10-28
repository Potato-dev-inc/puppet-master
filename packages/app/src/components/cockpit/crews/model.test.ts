import { describe, expect, it } from 'vitest';
import type { Crew, CrewStartOutcome } from '../../../lib/crews-api';
import { blockedReason, dropRole, failureText, renameRole, roleLine, setTask } from './model';

const crew = (): Crew => ({
  name: 'Review team',
  roles: [
    { name: 'Lead', agent_type: 'alpha', task: 'Plan {folder}' },
    { name: 'Checker', agent_type: 'beta', task: 'Check {folder}', parent: 'Lead' },
    { name: 'Reporter', agent_type: 'beta', task: 'Report', parent: 'Checker' },
  ],
});

describe('crew draft editing', () => {
  it('renaming a role moves the roles under it', () => {
    const next = renameRole(crew(), 1, 'Tester');
    expect(next.roles.map((r) => [r.name, r.parent])).toEqual([
      ['Lead', undefined],
      ['Tester', 'Lead'],
      ['Reporter', 'Tester'],
    ]);
  });

  it('leaving a helper out re-parents what was under it; the lead stays', () => {
    const next = dropRole(crew(), 1);
    expect(next.roles.map((r) => [r.name, r.parent])).toEqual([
      ['Lead', undefined],
      ['Reporter', 'Lead'],
    ]);
    expect(dropRole(crew(), 0)).toEqual(crew());
  });

  it('editing a task changes only that role', () => {
    const next = setTask(crew(), 2, 'New');
    expect(next.roles.map((r) => r.task)).toEqual(['Plan {folder}', 'Check {folder}', 'New']);
  });

  it('says what blocks Save', () => {
    expect(blockedReason(crew())).toBeNull();
    expect(blockedReason({ ...crew(), name: '  ' })).toBe('Give the crew a name.');
    expect(blockedReason(setTask(crew(), 1, ' '))).toBe('Write a task for Checker.');
    expect(blockedReason(renameRole(crew(), 1, 'lead'))).toBe('Two roles have the same name.');
    expect(blockedReason(renameRole(crew(), 2, ''))).toBe('Every role needs a name.');
  });

  it('describes where a role sits', () => {
    expect(roleLine(crew().roles[0])).toBe('lead');
    expect(roleLine(crew().roles[1])).toBe('under Lead');
  });
});

describe('failureText', () => {
  const outcome = (code: string, message: string): CrewStartOutcome => ({ started: [], failed: { role: 'Checker', code, message }, not_started: [] });

  it('uses the plain sentence for a known code, named by the role', () => {
    expect(failureText(outcome('AGENT_NOT_INSTALLED', 'raw'))).toBe("Checker isn't installed. It isn't on this PC yet, so nothing ran.");
  });

  it('falls back to the message, and is null when nothing failed', () => {
    expect(failureText(outcome('SOMETHING_ELSE', 'It broke.'))).toBe('It broke.');
    expect(failureText({ started: [], failed: null, not_started: [] })).toBeNull();
  });
});
