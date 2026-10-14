// The language rules for Simple mode (IDEAS-R8-PLAIN-MODE §6.1), as code the test can run.
import { ALLOWED_WORDS } from './word-list';

export const MAX_WORDS_PER_SENTENCE = 15;
export const MAX_SENTENCES_PER_CARD = 3;

/** Words (and two-word phrases) Simple never shows (§6.2, right column). */
export const NEVER_WORDS = new Set(
  (
    'repository repo repos workspace directory path worktree branch sandbox isolated merge discard commit apply ' +
    'checkpoint restore revert history diff patch localhost port url transcript prompt turn session ' +
    'approval permission allow allows allowed deny denied approve terminal pane pty tui console login auth ' +
    'quota tokens credits budget envelope exhausted error errors failed failure crashed exception opus sonnet ' +
    'model models effort install npm winget deploy ship host lint build typecheck git github'
  ).split(' '),
);
export const NEVER_PHRASES = ['log in', 'rate limit', 'exit code', 'tool call', 'dev server', 'run tests'];

const FILE_NAME = /\b[\w-]+\.(?:html|htm|css|js|csv|md|txt|png|json|xlsx|zip)\b/gi;

/** Sentences of a card. A file name keeps its dot (it is not the end of a sentence). */
export function sentencesOf(text: string): string[] {
  return text
    .replace(FILE_NAME, 'file')
    .split(/(?<=[.!?…])\s+|\n+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** Letters-only words of a sentence, lower case; numbers and times are not words. */
export function wordsOf(sentence: string): string[] {
  return (sentence.replace(FILE_NAME, 'file').match(/[A-Za-z]+(?:['’][A-Za-z]+)?/g) ?? []).map((w) => w.toLowerCase().replace('’', "'"));
}

const count = (sentence: string) => sentence.replace(FILE_NAME, 'file').split(/\s+/).filter((t) => /[A-Za-z0-9]/.test(t)).length;

export interface Problem {
  text: string;
  reason: string;
}

/** Everything wrong with one Simple string (empty when it is fine). */
export function problemsIn(text: string, allowed: Set<string> = ALLOWED_WORDS): Problem[] {
  const out: Problem[] = [];
  const sentences = sentencesOf(text);
  if (sentences.length > MAX_SENTENCES_PER_CARD) {
    out.push({ text, reason: `${sentences.length} sentences (at most ${MAX_SENTENCES_PER_CARD})` });
  }
  for (const sentence of sentences) {
    const n = count(sentence);
    if (n > MAX_WORDS_PER_SENTENCE) out.push({ text: sentence, reason: `${n} words (at most ${MAX_WORDS_PER_SENTENCE})` });
    for (const word of wordsOf(sentence)) {
      if (NEVER_WORDS.has(word)) out.push({ text: sentence, reason: `"${word}" is a word Simple never uses` });
      else if (!allowed.has(word)) out.push({ text: sentence, reason: `"${word}" is not on the word list` });
    }
  }
  const flat = wordsOf(text).join(' ');
  for (const phrase of NEVER_PHRASES) {
    if (` ${flat} `.includes(` ${phrase} `)) out.push({ text, reason: `"${phrase}" is a phrase Simple never uses` });
  }
  if (/!/.test(text)) out.push({ text, reason: 'no exclamation marks' });
  return out;
}
