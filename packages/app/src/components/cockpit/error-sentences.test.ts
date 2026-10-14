import { describe, expect, it } from 'vitest';
import { explainError, unknownErrorSentence } from './error-sentences';

const CODES = ['WORKSPACE_TRUST_DECLINED', 'AGENT_EXITED', 'AGENT_NOT_INSTALLED', 'PANE_NOT_READY', 'PROMPT_NOT_SUBMITTED'];

describe('explainError', () => {
  it.each(CODES)('%s has a sentence and a fix button, and the code is not in the text', (code) => {
    const e = explainError(code, 'Codex');
    expect(e).not.toBeNull();
    expect(e!.sentence.length).toBeGreaterThan(10);
    expect(e!.fixLabel.length).toBeGreaterThan(0);
    expect(e!.sentence).toContain('Codex');
    expect(e!.sentence).not.toMatch(/[A-Z]{3,}_[A-Z_]+/);
    expect(e!.sentence).not.toMatch(/exit code|\bpane id\b/i);
  });
  it('uses the sentences and buttons of the Phase 1 table', () => {
    expect(explainError('WORKSPACE_TRUST_DECLINED', 'Claude Code')).toEqual({
      sentence: "Claude Code closed because you chose Don't trust. The task wasn't sent.",
      fix: 'start_again',
      fixLabel: 'Start again',
    });
    expect(explainError('AGENT_NOT_INSTALLED', 'Claude Code')?.fixLabel).toBe('Install');
    expect(explainError('PANE_NOT_READY', 'Codex')?.fixLabel).toBe('Open pane');
  });
  it('has nothing for an unknown code or none, and the fallback never says "terminal"', () => {
    expect(explainError('SOMETHING_NEW', 'Codex')).toBeNull();
    expect(explainError(null, 'Codex')).toBeNull();
    expect(unknownErrorSentence('Codex')).not.toMatch(/terminal/i);
  });
});
