// The end-of-Focus digest has ONE shape. Rust writes it (`turn_summary::digest::DigestFocus`, golden file
// `turn_summary/golden/digest_focus.json`); `DigestFocus` in cockpit-api.ts reads it. If either side changes, this
// fails until both agree.
import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import type { DigestFocus } from './cockpit-api';

const GOLDEN = path.resolve(__dirname, '../../src-tauri/src/turn_summary/golden/digest_focus.json');

// Every key of the TypeScript type, with a value of the right kind. A key added to the type must be added here.
const SHAPE: DigestFocus = {
  line: '',
  minutes: 0,
  endedBy: 'timer',
  counts: { turnsDone: 0, agents: 0, failed: 0, asked: 0, urgent: 0, stillWaiting: 0, held: 0 },
};

function kinds(value: unknown): unknown {
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([k, v]) => [k, kinds(v)]));
  }
  return typeof value;
}

describe('Digest.focus wire shape', () => {
  const golden = JSON.parse(fs.readFileSync(GOLDEN, 'utf8')) as DigestFocus;

  it('has exactly the keys and kinds the card reads', () => {
    expect(kinds(golden)).toEqual(kinds(SHAPE));
  });

  it('counts.held is one number and endedBy is a word', () => {
    expect(typeof golden.counts.held).toBe('number');
    expect(['timer', 'early']).toContain(golden.endedBy);
  });
});
