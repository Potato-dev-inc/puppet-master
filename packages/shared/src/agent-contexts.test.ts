import { describe, expect, it } from 'vitest';
import { getAgentContextProfile } from './agent-contexts';

describe('agent context profiles', () => {
  it('keeps the Cursor IDE and Cursor Agent CLI profiles distinct', () => {
    const ide = getAgentContextProfile('cursor');
    const agent = getAgentContextProfile('cursor_agent');

    expect(ide.label).toBe('Cursor IDE');
    expect(ide.strengths).toContain('ui-orchestration');
    expect(agent.label).toBe('Cursor Agent CLI');
    expect(agent.strengths).toContain('codebase-reasoning');
    expect(agent.context_notes.join(' ')).toMatch(/plan mode/i);
  });
});
