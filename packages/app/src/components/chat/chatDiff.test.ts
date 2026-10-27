import { describe, expect, it } from 'vitest';
import { editPairs, lineDiff, patchLines, patchOf } from './chatDiff';

describe('lineDiff', () => {
  it('keeps the common lines and marks the changed ones', () => {
    expect(lineDiff('a\nb\nc', 'a\nB\nc')).toEqual([
      { op: 'same', text: 'a' },
      { op: 'del', text: 'b' },
      { op: 'add', text: 'B' },
      { op: 'same', text: 'c' },
    ]);
  });

  it('adds every line of a new file and removes every line of an empty one', () => {
    expect(lineDiff('', 'x\ny')).toEqual([
      { op: 'add', text: 'x' },
      { op: 'add', text: 'y' },
    ]);
    expect(lineDiff('x', '')).toEqual([{ op: 'del', text: 'x' }]);
  });

  it('gives one block for a very large change instead of a long table', () => {
    const big = Array.from({ length: 400 }, (_, i) => `old ${i}`).join('\n');
    const next = Array.from({ length: 400 }, (_, i) => `new ${i}`).join('\n');
    const lines = lineDiff(big, next);
    expect(lines).toHaveLength(800);
    expect(lines.slice(0, 400).every((line) => line.op === 'del')).toBe(true);
  });
});

describe('editPairs', () => {
  it('reads one old and new pair, or a list of them', () => {
    expect(editPairs({ file_path: 'a.ts', old_string: 'x', new_string: 'y' })).toEqual([{ before: 'x', after: 'y' }]);
    expect(editPairs({ edits: [{ old_string: 'p', new_string: 'q' }, { oldString: 'r', newString: '' }] })).toEqual([
      { before: 'p', after: 'q' },
      { before: 'r', after: '' },
    ]);
  });

  it('reads a write as all added text, and gives null for other input', () => {
    expect(editPairs({ file_path: 'new.txt', content: 'hello' })).toEqual([{ before: '', after: 'hello' }]);
    expect(editPairs({ command: 'ls' })).toBeNull();
    expect(editPairs('text')).toBeNull();
    expect(editPairs({ edits: [] })).toBeNull();
  });
});

describe('patch text', () => {
  it('reads the patch of an input or the input itself', () => {
    expect(patchOf({ patch: '*** Begin Patch' })).toBe('*** Begin Patch');
    expect(patchOf('raw')).toBe('raw');
    expect(patchOf({ command: 'x' })).toBeNull();
  });

  it('marks the plus and minus lines and the headers of a patch', () => {
    const lines = patchLines('*** Update File: a.ts\n@@\n-old\n+new\n keep');
    expect(lines.map((line) => line.op)).toEqual(['head', 'head', 'del', 'add', 'same']);
    expect(lines[3]).toEqual({ op: 'add', text: 'new' });
    expect(lines[4]).toEqual({ op: 'same', text: 'keep' });
  });
});
