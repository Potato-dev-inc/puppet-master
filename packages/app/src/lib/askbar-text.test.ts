import { describe, expect, it } from 'vitest';
import { splitBlocks } from './askbar-text';

describe('splitBlocks', () => {
  it('returns one trimmed text block when there are no fences', () => {
    expect(splitBlocks('  Use the cache.\n\nIt is faster.  \n')).toEqual([{ kind: 'text', text: 'Use the cache.\n\nIt is faster.' }]);
  });

  it('returns nothing for an empty or whitespace-only answer', () => {
    expect(splitBlocks('')).toEqual([]);
    expect(splitBlocks('   \n  ')).toEqual([]);
  });

  it('splits prose around a fenced block and reads the language', () => {
    const answer = 'Run this:\n```ts\nconst a = 1;\n```\nThen check.';
    expect(splitBlocks(answer)).toEqual([
      { kind: 'text', text: 'Run this:' },
      { kind: 'code', text: 'const a = 1;', lang: 'ts' },
      { kind: 'text', text: 'Then check.' },
    ]);
  });

  it('drops empty text blocks between adjacent fences and keeps a language-less fence', () => {
    const answer = '```bash\nnpm test\n```\n\n```\nplain\n```';
    expect(splitBlocks(answer)).toEqual([
      { kind: 'code', text: 'npm test', lang: 'bash' },
      { kind: 'code', text: 'plain', lang: '' },
    ]);
  });

  it('runs an unterminated fence to the end of the answer', () => {
    const answer = 'Here:\n```rust\nfn main() {\n    println!("hi");\n}';
    expect(splitBlocks(answer)).toEqual([
      { kind: 'text', text: 'Here:' },
      { kind: 'code', text: 'fn main() {\n    println!("hi");\n}', lang: 'rust' },
    ]);
  });
});
