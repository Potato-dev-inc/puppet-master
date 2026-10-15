// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { reviewedText } from './second-opinion';

describe('reviewedText', () => {
  it('says "this separate copy" for a copy, never "the worktree changes"', () => {
    expect(reviewedText({ source: 'worktree', sourceLabel: 'the worktree changes' })).toBe('this separate copy');
  });

  it('keeps the backend wording for the other sources', () => {
    expect(reviewedText({ source: 'checkpoint', sourceLabel: 'the last turn' })).toBe('the last turn');
    expect(reviewedText({ source: 'git', sourceLabel: 'uncommitted changes in the project' })).toBe('uncommitted changes in the project');
  });
});
