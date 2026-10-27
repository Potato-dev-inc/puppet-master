import { describe, expect, it } from 'vitest';
import { classifyTool, toolKeyOf, TOOL_KIND_NAMES } from './chatToolKinds';

describe('classifyTool', () => {
  it('maps the listed names of each kind', () => {
    expect(classifyTool({ name: 'Read' })).toBe('read');
    expect(classifyTool({ name: 'Grep' })).toBe('search');
    expect(classifyTool({ name: 'Edit' })).toBe('edit');
    expect(classifyTool({ name: 'apply_patch' })).toBe('edit');
    expect(classifyTool({ name: 'Write' })).toBe('write');
    expect(classifyTool({ name: 'Bash' })).toBe('run');
    expect(classifyTool({ name: 'exec' })).toBe('run');
    expect(classifyTool({ name: 'WebFetch' })).toBe('web');
    expect(classifyTool({ name: 'web_search' })).toBe('web');
  });

  it('ignores case and a namespace prefix', () => {
    expect(classifyTool({ name: 'BASH' })).toBe('run');
    expect(classifyTool({ name: 'mcp__files__read' })).toBe('read');
    expect(classifyTool({ name: 'server.grep' })).toBe('search');
  });

  it('marks a helper by its flag before any name', () => {
    expect(classifyTool({ name: 'Read', helper: true })).toBe('helper');
    expect(classifyTool({ name: 'Task' })).toBe('helper');
  });

  it('falls back to the input: a command runs, a patch or edits list edit', () => {
    expect(classifyTool({ name: 'shell_new', input: { command: 'npm test' } })).toBe('run');
    expect(classifyTool({ name: 'patcher', input: { patch: '*** Begin Patch' } })).toBe('edit');
    expect(classifyTool({ name: 'multi', input: { edits: [] } })).toBe('edit');
  });

  it('counts a screen line that starts with a dollar sign as a run, and an unnamed row with writes as an edit', () => {
    expect(classifyTool({ name: '', summary: '$ ls -la' })).toBe('run');
    expect(classifyTool({ name: '', summary: 'some text', writes: true })).toBe('edit');
  });

  it('gives any other tool the kind other', () => {
    expect(classifyTool({ name: 'TodoWrite' })).toBe('other');
    expect(classifyTool({ name: '' })).toBe('other');
  });

  it('keeps every name in exactly one kind', () => {
    const all = Object.values(TOOL_KIND_NAMES).flat();
    expect(new Set(all).size).toBe(all.length);
  });
});

describe('toolKeyOf', () => {
  it('lower-cases and drops a namespace prefix', () => {
    expect(toolKeyOf(' Mcp__x__Glob ')).toBe('glob');
    expect(toolKeyOf('')).toBe('');
  });
});
