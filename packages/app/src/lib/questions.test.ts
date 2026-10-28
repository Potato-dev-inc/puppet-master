import { describe, expect, it } from 'vitest';
import { type Question, isIssueCard, isIssuePush, isIssueStart } from './questions';

function question(over: Partial<Question> = {}): Question {
  return {
    id: 'q-1',
    question: 'Start on #42 "Login button does nothing" with Codex?',
    choices: [{ id: 'start', label: 'Start with Codex' }],
    allow_free_text: false,
    urgency: 'normal',
    created_at_ms: 1_000,
    expires_at_ms: 2_000,
    state: 'pending',
    ...over,
  };
}

describe('issue cards', () => {
  it('recognises the start card', () => {
    const q = question({ kind: 'issue_start' });
    expect(isIssueStart(q)).toBe(true);
    expect(isIssuePush(q)).toBe(false);
    expect(isIssueCard(q)).toBe(true);
  });

  it('recognises the push card', () => {
    const q = question({ kind: 'issue_push' });
    expect(isIssuePush(q)).toBe(true);
    expect(isIssueStart(q)).toBe(false);
    expect(isIssueCard(q)).toBe(true);
  });

  it('does not treat other cards as issue cards', () => {
    expect(isIssueCard(question())).toBe(false);
    expect(isIssueCard(question({ kind: 'budget' }))).toBe(false);
    expect(isIssueCard(question({ kind: 'issue_start_extra' }))).toBe(false);
  });
});
