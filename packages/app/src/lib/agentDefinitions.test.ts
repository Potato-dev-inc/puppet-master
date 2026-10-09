import { describe, expect, it } from 'vitest';
import { argvDiff, canReview, type DefinitionRow } from './agentDefinitions';

const row = (over: Partial<DefinitionRow>): DefinitionRow => ({
  id: 'x',
  file: 'C:/a/x.toml',
  source: 'user',
  state: 'active',
  errors: [],
  fingerprint: 'sha256:1',
  shadowed_by: null,
  ...over,
});

describe('argvDiff', () => {
  it('marks added and removed arguments and keeps the rest', () => {
    const lines = argvDiff(['run', '--json', '<task>'], ['run', '--yolo', '--json', '<task>']);
    expect(lines.map((l) => `${l.kind}:${l.text}`)).toEqual([
      'same:run',
      'added:--yolo',
      'same:--json',
      'same:<task>',
    ]);
    const removed = argvDiff(['a', 'b'], ['a']);
    expect(removed.map((l) => `${l.kind}:${l.text}`)).toEqual(['same:a', 'removed:b']);
  });

  it('is all same for equal argv and all added for an empty approval', () => {
    expect(argvDiff(['a'], ['a']).every((l) => l.kind === 'same')).toBe(true);
    expect(argvDiff([], ['a', 'b']).every((l) => l.kind === 'added')).toBe(true);
  });
});

describe('canReview', () => {
  it('allows review for new, changed and binary-swapped definitions only', () => {
    expect(canReview(row({ state: 'needs_approval' }))).toBe(true);
    expect(canReview(row({ state: 'approval_stale' }))).toBe(true);
    expect(canReview(row({ state: 'active' }))).toBe(false);
    expect(canReview(row({ state: 'active', binary_changed: true }))).toBe(true);
    expect(canReview(row({ state: 'invalid', fingerprint: null }))).toBe(false);
    expect(canReview(row({ state: 'shadowed' }))).toBe(false);
  });
});
