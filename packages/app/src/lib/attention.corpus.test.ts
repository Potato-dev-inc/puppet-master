// The parity corpus (packages/shared/fixtures/attention/*.json) pins `deriveNeedsYou` to the behaviour
// that shipped. The Rust port (src-tauri/src/attention/derive.rs) runs the SAME files in
// `attention::tests::parity`, so the two implementations cannot drift apart unnoticed.
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { blockingItems, deriveNeedsYou, type AttentionInput } from './attention';

const dir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../shared/fixtures/attention');

interface Case {
  name: string;
  note: string;
  input: Omit<AttentionInput, 'statusSince'> & { statusSince?: Record<string, number> };
  expected: unknown[];
  /** Ids that count (sorted): the store's rules applied to `expected`. */
  blocking: string[];
}

const files = readdirSync(dir).filter((f) => f.endsWith('.json')).sort();

describe('attention parity corpus', () => {
  it('has the cases the Rust side expects', () => {
    expect(files.length).toBeGreaterThanOrEqual(20);
  });

  for (const file of files) {
    const c = JSON.parse(readFileSync(path.join(dir, file), 'utf8')) as Case;
    it(`${file}: ${c.note}`, () => {
      const statusSince = c.input.statusSince ? new Map(Object.entries(c.input.statusSince)) : undefined;
      const items = deriveNeedsYou({ ...c.input, statusSince });
      // JSON has no `undefined`: compare what a client would see on the wire.
      expect(JSON.parse(JSON.stringify(items))).toEqual(c.expected);
    });

    it(`${file}: what counts`, () => {
      const statusSince = c.input.statusSince ? new Map(Object.entries(c.input.statusSince)) : undefined;
      const counted = blockingItems(deriveNeedsYou({ ...c.input, statusSince })).map((i) => i.id).sort();
      expect(counted).toEqual(c.blocking);
    });
  }
});
