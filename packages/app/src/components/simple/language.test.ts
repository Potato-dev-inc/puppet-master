// Every string Simple mode shows follows the language rules: sentences of 15 words or fewer, at most
// 3 sentences in a card, only words from the fixed list, none of the words Simple never uses.
import { describe, expect, it } from 'vitest';
import { STRINGS as APPROVAL } from './approval-text';
import { ownStrings } from './copy';
import { STRINGS as PROBLEMS } from './error-text';
import { MAX_SENTENCES_PER_CARD, MAX_WORDS_PER_SENTENCE, NEVER_WORDS, problemsIn, sentencesOf, wordsOf } from './language-rules';
import { STRINGS as TEMPLATES } from './templates';
import { ALLOWED_WORDS } from './word-list';

const ALL = [...ownStrings(), ...APPROVAL, ...PROBLEMS, ...TEMPLATES];

describe('Simple mode language', () => {
  it('checks a real number of strings', () => {
    expect(ALL.length).toBeGreaterThan(150);
  });

  it('every string follows the rules', () => {
    const bad = ALL.flatMap((text) => problemsIn(text).map((p) => `${p.reason}: ${p.text}`));
    expect([...new Set(bad)]).toEqual([]);
  });

  it('the checker itself catches what it should', () => {
    const long = 'Claude Code wants to look at all of the files that you have in this project right now today.';
    expect(problemsIn(long).some((p) => p.reason.includes('words'))).toBe(true);
    expect(problemsIn('It is one. It is two. It is three. It is four.').some((p) => p.reason.includes('sentences'))).toBe(true);
    expect(problemsIn('Claude Code made a commit to the branch.').map((p) => p.reason).join()).toMatch(/commit/);
    expect(problemsIn('The checkpoint failed.').map((p) => p.reason).join()).toMatch(/checkpoint/);
    expect(problemsIn('Press the xylophone.').map((p) => p.reason).join()).toMatch(/not on the word list/);
    expect(problemsIn('Do it now!').map((p) => p.reason).join()).toMatch(/exclamation/);
    expect(problemsIn('Your exit code is 1.').map((p) => p.reason).join()).toMatch(/code/);
    expect(problemsIn('Press Keep. Your work is safe.')).toEqual([]);
  });

  it('a file name does not end a sentence and does not count as a word to look up', () => {
    expect(sentencesOf('Claude Code made quiz.html. It works.')).toHaveLength(2);
    expect(wordsOf('Made quiz.html.')).toEqual(['made', 'file']);
  });

  it('the limits are the ones in the design', () => {
    expect(MAX_WORDS_PER_SENTENCE).toBe(15);
    expect(MAX_SENTENCES_PER_CARD).toBe(3);
  });

  it('no word is on both lists', () => {
    expect([...NEVER_WORDS].filter((w) => ALLOWED_WORDS.has(w))).toEqual([]);
  });
});
