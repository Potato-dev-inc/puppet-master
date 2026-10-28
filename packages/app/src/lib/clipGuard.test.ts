import { describe, expect, it } from 'vitest';
import { GuardCounter, guardText } from './clipGuard';

// Fake secrets are built here at run time, never written out as literal text.
const FAKE: Record<string, string> = {
  apiKey: `sk-ant-api03-${'A1'.repeat(20)}`,
  githubToken: `ghp_${'a'.repeat(36)}`,
  githubPat: `github_pat_${'B2'.repeat(20)}`,
  awsKey: `AKIA${'Z'.repeat(16)}`,
  slackToken: `xoxb-${'1'.repeat(12)}-${'a'.repeat(12)}`,
  jwt: 'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl',
  privateKey: `-----BEGIN PRIVATE KEY-----\n${'Q'.repeat(40)}`,
  email: 'person@example.test',
  windowsHome: 'C:\\Users\\you\\proj\\a.rs',
  linuxHome: '/home/you/proj/a.rs',
  macHome: '/Users/you/proj/a.rs',
  longRun: 'x'.repeat(30),
};

describe('guardText hides each shape', () => {
  for (const [name, sample] of Object.entries(FAKE)) {
    it(`hides ${name}`, () => {
      const r = guardText(sample);
      expect(r.hit).toBe(true);
      expect(r.text).toContain('[hidden]');
      expect(r.text).not.toContain(sample);
    });
  }

  it('hides only the secret inside a sentence', () => {
    expect(guardText(`use ${FAKE.apiKey} now`)).toEqual({ text: 'use [hidden] now', hit: true });
  });

  it('hides a whole Windows home path, not just its start', () => {
    expect(guardText(FAKE.windowsHome)).toEqual({ text: '[hidden]', hit: true });
  });
});

describe('guardText leaves normal text alone', () => {
  it.each([
    'Read 6 files · Ran 2 commands',
    'Fixed the login bug in auth.ts.',
    'Edited packages/app/src/lib/clipGuard.test.ts (+12 -3)',
    'risk-based task-runner',
    'Ran 3 commands · 1 failed',
    '',
  ])('keeps %j', (text) => {
    expect(guardText(text)).toEqual({ text, hit: false });
  });

  it('keeps a 29-character run and hides a 30-character run', () => {
    const run29 = 'a'.repeat(29);
    expect(guardText(run29)).toEqual({ text: run29, hit: false });
    expect(guardText(FAKE.longRun)).toEqual({ text: '[hidden]', hit: true });
  });
});

describe('GuardCounter', () => {
  it('returns guarded text and counts only the texts that had a hit', () => {
    const guard = new GuardCounter();
    expect(guard.hits).toBe(0);
    expect(guard.guard('Read 6 files')).toBe('Read 6 files');
    expect(guard.guard(`key ${FAKE.apiKey}`)).toBe('key [hidden]');
    expect(guard.guard(FAKE.email)).toBe('[hidden]');
    expect(guard.guard('Ran 2 commands')).toBe('Ran 2 commands');
    expect(guard.hits).toBe(2);
  });
});
