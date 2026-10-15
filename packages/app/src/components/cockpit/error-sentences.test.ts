import { describe, expect, it } from 'vitest';
import { explainError, unknownError } from './error-sentences';

const CODES = ['WORKSPACE_TRUST_DECLINED', 'AGENT_EXITED', 'AGENT_NOT_INSTALLED', 'PANE_NOT_READY', 'PROMPT_NOT_SUBMITTED'];

describe('explainError', () => {
  it.each(CODES)('%s has a title, a sentence and a fix button, and the code is not in the text', (code) => {
    const e = explainError(code, 'Codex');
    expect(e).not.toBeNull();
    expect(e!.sentence.length).toBeGreaterThan(10);
    expect(e!.fixLabel.length).toBeGreaterThan(0);
    // what happened (the title, with the agent's name), what is safe or what to do (the sentence)
    expect(e!.title).toContain('Codex');
    const text = `${e!.title} ${e!.sentence}`;
    expect(text).not.toMatch(/[A-Z]{3,}_[A-Z_]+/);
    expect(text).not.toMatch(/exit code|\bpane\b/i);
  });

  it('says what happened, what is safe and one next step', () => {
    expect(explainError('WORKSPACE_TRUST_DECLINED', 'Claude Code')).toEqual({
      title: "Claude Code didn't start",
      sentence: "You chose not to trust this folder, so the task wasn't sent. Start again to be asked once more.",
      fix: 'start_again',
      fixLabel: 'Start again',
      // the person's own choice is not drawn as a failure: the calm glyph, not the (x)
      glyph: 'notice',
    });
    expect(explainError('AGENT_EXITED', 'Codex')).toMatchObject({ title: 'Codex stopped', glyph: 'error' });
    expect(explainError('AGENT_EXITED', 'Codex')?.sentence).toContain('Your files are as it left them.');
    expect(explainError('AGENT_NOT_INSTALLED', 'Claude Code')?.fixLabel).toBe('Install');
    expect(explainError('PANE_NOT_READY', 'Codex')?.fixLabel).toBe('Open');
  });

  it('never offers to trust a folder for you: starting again asks again', () => {
    const e = explainError('WORKSPACE_TRUST_DECLINED', 'Codex')!;
    expect(e.fixLabel).not.toMatch(/trust/i);
    expect(e.sentence).toMatch(/asked once more/);
  });

  it('has nothing for an unknown code or none, and the fallback never says "terminal"', () => {
    expect(explainError('SOMETHING_NEW', 'Codex')).toBeNull();
    expect(explainError(null, 'Codex')).toBeNull();
    expect(unknownError('Codex').sentence).not.toMatch(/terminal/i);
    expect(unknownError('Codex').title).toBe('Codex stopped');
    expect(unknownError('Codex').sentence).toContain('Your files are as it left them.');
  });
});
