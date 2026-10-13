/** Helpers for showing a unified diff in the monochrome Changes viewer. */

export type DiffLineKind = 'add' | 'del' | 'hunk' | 'ctx';

export function lineKind(line: string): DiffLineKind {
  if (line.startsWith('@@')) return 'hunk';
  if (line.startsWith('+')) return 'add';
  if (line.startsWith('-')) return 'del';
  return 'ctx';
}

/** Split a `git diff` patch into per-file body lines (headers dropped), keyed by path. */
export function splitPatch(patch: string): Map<string, string[]> {
  const files = new Map<string, string[]>();
  let current: string[] | null = null;
  let inBody = false;
  for (const line of patch.split('\n')) {
    const header = /^diff --git a\/(.+?) b\/(.+)$/.exec(line);
    if (header) {
      current = [];
      files.set(header[2], current);
      inBody = false;
      continue;
    }
    if (!current) continue;
    if (!inBody) {
      if (line.startsWith('@@')) {
        inBody = true;
        current.push(line);
      } else if (line.startsWith('Binary files')) {
        current.push(line);
      }
      continue;
    }
    current.push(line);
  }
  for (const lines of files.values()) {
    while (lines.length > 0 && lines[lines.length - 1] === '') lines.pop();
  }
  return files;
}

const STATUS_MARK = { added: 'A', modified: 'M', deleted: 'D' } as const;
export function statusMark(status: 'added' | 'modified' | 'deleted'): string {
  return STATUS_MARK[status];
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
