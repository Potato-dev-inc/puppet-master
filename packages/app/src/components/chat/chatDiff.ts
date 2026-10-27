// Line diffs for the tool detail: an edit's old and new text, a write's new text, or an agent's patch. Pure code.
// Red and green are not used: the view shows `+` and `-` in a gutter, and marks the lines by weight and fill.

export type DiffOp = 'same' | 'add' | 'del' | 'head';

export interface DiffLine {
  op: DiffOp;
  text: string;
}

/** Past this many table cells the diff is one block: all removed, then all added. */
const MAX_CELLS = 40_000;

const linesOf = (text: string): string[] => (text === '' ? [] : text.split('\n'));

/** Lines in `before` but not `after` are removed, lines in `after` but not `before` are added (longest common run). */
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = linesOf(before);
  const b = linesOf(after);
  const n = a.length;
  const m = b.length;
  if (n * m > MAX_CELLS) {
    return [...a.map((text) => ({ op: 'del' as const, text })), ...b.map((text) => ({ op: 'add' as const, text }))];
  }
  // table[i * width + j]: the longest common run of a[i..] and b[j..].
  const width = m + 1;
  const table = new Uint32Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      table[i * width + j] =
        a[i] === b[j] ? table[(i + 1) * width + j + 1]! + 1 : Math.max(table[(i + 1) * width + j]!, table[i * width + j + 1]!);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ op: 'same', text: a[i]! });
      i++;
      j++;
    } else if (table[(i + 1) * width + j]! >= table[i * width + j + 1]!) {
      out.push({ op: 'del', text: a[i]! });
      i++;
    } else {
      out.push({ op: 'add', text: b[j]! });
      j++;
    }
  }
  while (i < n) out.push({ op: 'del', text: a[i++]! });
  while (j < m) out.push({ op: 'add', text: b[j++]! });
  return out;
}

export interface EditPair {
  before: string;
  after: string;
}

const BEFORE_KEYS = ['old_string', 'oldString', 'old_text', 'oldText'];
const AFTER_KEYS = ['new_string', 'newString', 'new_text', 'newText'];

function firstString(rec: Record<string, unknown>, keys: readonly string[]): string | null {
  for (const key of keys) {
    const value = rec[key];
    if (typeof value === 'string') return value;
  }
  return null;
}

function pairOf(value: unknown): EditPair | null {
  if (!value || typeof value !== 'object') return null;
  const rec = value as Record<string, unknown>;
  const after = firstString(rec, AFTER_KEYS);
  if (after === null) return null;
  return { before: firstString(rec, BEFORE_KEYS) ?? '', after };
}

/**
 * The old and new text of an edit call: one pair (`old_string`, `new_string`), a list (`edits`), or a write's
 * `content` (all added). Null when the input has none of these.
 */
export function editPairs(input: unknown): EditPair[] | null {
  if (!input || typeof input !== 'object') return null;
  const rec = input as Record<string, unknown>;
  if (Array.isArray(rec.edits)) {
    const pairs = rec.edits.map(pairOf).filter((pair): pair is EditPair => pair !== null);
    return pairs.length > 0 ? pairs : null;
  }
  const pair = pairOf(rec);
  if (pair) return [pair];
  if (typeof rec.content === 'string') return [{ before: '', after: rec.content }];
  return null;
}

/** The patch text of an edit call: the input itself, or its `patch` field. */
export function patchOf(input: unknown): string | null {
  if (typeof input === 'string') return input;
  if (input && typeof input === 'object') {
    const patch = (input as Record<string, unknown>).patch;
    if (typeof patch === 'string') return patch;
  }
  return null;
}

/** An agent's patch text as diff lines: plus and minus lines, star and at headers, the rest as context. */
export function patchLines(patch: string): DiffLine[] {
  return patch.split('\n').map((line): DiffLine => {
    if (line.startsWith('*** ') || line.startsWith('@@')) return { op: 'head', text: line };
    if (line.startsWith('+')) return { op: 'add', text: line.slice(1) };
    if (line.startsWith('-')) return { op: 'del', text: line.slice(1) };
    return { op: 'same', text: line.startsWith(' ') ? line.slice(1) : line };
  });
}
