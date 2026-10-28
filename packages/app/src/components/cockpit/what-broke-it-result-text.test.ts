import { describe, expect, it } from 'vitest';
import { CULPRIT, NOT_SURE_FLAKY, NOT_SURE_RUNS, TESTS_PASS } from '../../dev/what-broke-it-result-fixtures';
import {
  brokeLine,
  canKeepLooking,
  countFiles,
  detailsText,
  maskPaths,
  recheckLine,
  undoRefusedText,
  undoText,
} from './what-broke-it-result-text';

describe('what-broke-it-result-text', () => {
  it('names one failing test, or the first one and how many more', () => {
    expect(brokeLine([])).toBe('No test name was printed.');
    expect(brokeLine(['cart.test.ts'])).toBe('It broke cart.test.ts');
    expect(brokeLine(['a.test.ts', 'b.test.ts', 'c.test.ts'])).toBe('It broke a.test.ts and 2 more');
  });

  it('uses the same refusal wording for none, one and several files', () => {
    expect(undoRefusedText(['cart.ts'])).toBe(
      "Can't undo only this turn. A later change touched the same lines in cart.ts. Nothing was changed.",
    );
    expect(undoRefusedText(['cart.ts', 'a.ts', 'b.ts'])).toBe(
      "Can't undo only this turn. A later change touched the same lines in cart.ts and 2 more. Nothing was changed.",
    );
  });

  it('says "the agent" for an unknown label and "its files" when the count is missing', () => {
    expect(undoText(3, null, 'An agent')).toContain('changed in its files.');
    expect(undoText(3, null, 'An agent')).toContain("It doesn't change what the agent remembers.");
  });

  it('counts files with singular and plural', () => {
    expect(countFiles(1)).toBe('1 file');
    expect(countFiles(4)).toBe('4 files');
  });

  it('offers Keep looking only when the search ran out of runs or time', () => {
    expect(canKeepLooking(NOT_SURE_RUNS)).toBe(true);
    expect(canKeepLooking({ kind: 'not_sure', title: 'Not sure. Ran out of time.' })).toBe(true);
    expect(canKeepLooking(NOT_SURE_FLAKY)).toBe(false);
    expect(canKeepLooking(CULPRIT)).toBe(false);
  });

  it('describes the recheck result in plain words', () => {
    expect(recheckLine(TESTS_PASS)).toBe('The tests pass now.');
    expect(recheckLine({ kind: 'culprit', title: 'x', failing: ['a.test.ts', 'b.test.ts'] })).toBe(
      'They still fail: a.test.ts, b.test.ts',
    );
  });

  it('masks Windows and home folder paths', () => {
    expect(maskPaths('at C:\\Users\\you\\dev\\cart.test.ts:14 done')).toBe('at ~ done');
    expect(maskPaths('at /home/you/dev/cart.js')).toBe('at ~');
  });

  it('copy details has the title, the turn and the tail, with no folder path', () => {
    const text = detailsText({ ...CULPRIT, tail: 'at C:\\Users\\you\\dev\\cart\\cart.test.ts:14' });
    expect(text.split('\n')[0]).toBe('Turn 7 by Codex broke the tests');
    expect(text).toContain('Codex · turn 7: refactor cart');
    expect(text).toContain('Last lines of the failing run:');
    expect(text).not.toContain('Users');
  });
});
