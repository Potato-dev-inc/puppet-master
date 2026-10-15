// @vitest-environment node
// The welcome speaks plain English: short sentences, short cards, none of the words the UX word list bans.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { MAX_SENTENCES_PER_CARD, MAX_WORDS_PER_SENTENCE, sentencesOf, wordsOf } from '../simple/language-rules';
import { ownStrings, T, TASK_PROMPT } from './copy';

const here = path.dirname(fileURLToPath(import.meta.url));

/** UX-PHASE1 section 0.3, right column: the engine's words. */
const BANNED = ['worktree', 'isolated', 'account', 'profile', 'subscription', 'quota', 'envelope', 'handle', 'doctor', 'hooks', 'checkpoint', 'restore', 'commit', 'branch', 'sandbox', 'ship'];

describe('first-run copy', () => {
  const all = ownStrings();

  it('checks a real number of strings', () => {
    expect(all.length).toBeGreaterThan(60);
  });

  it('every sentence is 15 words or fewer, and every card 3 sentences or fewer', () => {
    const bad = all.flatMap((text) => {
      const out: string[] = [];
      const sentences = sentencesOf(text);
      if (sentences.length > MAX_SENTENCES_PER_CARD) out.push(`${sentences.length} sentences: ${text}`);
      for (const s of sentences) {
        const n = s.split(/\s+/).filter((t) => /[A-Za-z0-9]/.test(t)).length;
        if (n > MAX_WORDS_PER_SENTENCE) out.push(`${n} words: ${s}`);
      }
      return out;
    });
    expect(bad).toEqual([]);
  });

  it('none of the banned engine words appears', () => {
    const bad = all.flatMap((text) => wordsOf(text).filter((w) => BANNED.includes(w)).map((w) => `${w}: ${text}`));
    expect(bad).toEqual([]);
  });

  it('uses the words of the list: separate copy, Sign in, Set up agents, Undo, History, Receipt', () => {
    expect(T.looks.ai.hint).toContain('separate copy');
    expect(T.agentsHowTo).toContain('sign in');
    expect(T.undoTip).toContain('History');
    expect(T.undoTip).toContain('Undo');
    expect(T.receiptTip).toContain('receipt');
  });

  it('the three answers are the founder wording', () => {
    expect(T.looks.code.label).toBe('I write code');
    expect(T.looks.ai.label).toBe('I build with AI, not much code');
    expect(T.looks.none.label).toBe("I don't code");
  });

  it('shows the same suggested prompt Rust sends', () => {
    const rust = readFileSync(path.join(here, '../../../src-tauri/src/first_run/mod.rs'), 'utf8');
    expect(rust).toContain(`"${TASK_PROMPT}"`);
    expect(TASK_PROMPT).toContain('Change nothing');
  });
});
