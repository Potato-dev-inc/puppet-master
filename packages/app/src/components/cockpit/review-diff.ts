/** Pure helpers for pinning a second opinion's findings onto a diff. No React here. */

export type Severity = 'high' | 'medium' | 'low' | 'info';

export interface ReviewFinding {
  id: string;
  file: string;
  line: number | null;
  endLine?: number | null;
  severity: Severity;
  text: string;
}

const HUNK = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

/**
 * For each line of one file's diff body (as produced by splitPatch: starts at an '@@' line), the line
 * number in the NEW file, or null for hunk headers and deleted lines. Context and '+' lines advance the
 * counter; '-' lines do not; a '\' (no newline marker) line is null.
 */
export function newLineNumbers(lines: string[]): (number | null)[] {
  const out: (number | null)[] = [];
  let next: number | null = null;
  for (const line of lines) {
    if (line.startsWith('@@')) {
      const m = HUNK.exec(line);
      next = m ? Number(m[1]) : null;
      out.push(null);
      continue;
    }
    if (next === null || line.startsWith('\\')) {
      out.push(null);
      continue;
    }
    if (line.startsWith('-')) {
      out.push(null);
      continue;
    }
    out.push(next);
    next += 1;
  }
  return out;
}

/** Normalise a path a reviewer wrote: backslashes to '/', strip leading './', trim. */
export function normalizePath(p: string): string {
  let out = p.trim().replace(/\\/g, '/');
  while (out.startsWith('./')) out = out.slice(2);
  return out;
}

/**
 * Find the key in `files` that a finding's file refers to: exact match after normalizePath, else a
 * unique key that ends with '/' + normalized, or that normalized ends with '/' + key; else null.
 */
export function matchFile(file: string, files: string[]): string | null {
  const n = normalizePath(file);
  if (!n) return null;
  const exact = files.find((k) => normalizePath(k) === n);
  if (exact !== undefined) return exact;
  const candidates = files.filter((k) => {
    const nk = normalizePath(k);
    return nk.endsWith(`/${n}`) || n.endsWith(`/${nk}`);
  });
  return candidates.length === 1 ? candidates[0] : null;
}

/**
 * Group findings by matched diff file. A finding is anchored when its file matches and either its line is
 * null (shown above the file's first line) or its line is a new-file line number present in that file's
 * diff. Everything else is unanchored.
 */
export function anchorFindings(
  findings: ReviewFinding[],
  bodies: Map<string, string[]>,
): { byFile: Map<string, ReviewFinding[]>; unanchored: ReviewFinding[] } {
  const byFile = new Map<string, ReviewFinding[]>();
  const unanchored: ReviewFinding[] = [];
  const files = [...bodies.keys()];
  const lineSets = new Map<string, Set<number>>();
  for (const f of findings) {
    const key = matchFile(f.file, files);
    if (key === null) {
      unanchored.push(f);
      continue;
    }
    if (f.line !== null) {
      let set = lineSets.get(key);
      if (!set) {
        set = new Set(newLineNumbers(bodies.get(key) ?? []).filter((n): n is number => n !== null));
        lineSets.set(key, set);
      }
      if (!set.has(f.line)) {
        unanchored.push(f);
        continue;
      }
    }
    const list = byFile.get(key) ?? [];
    list.push(f);
    byFile.set(key, list);
  }
  return { byFile, unanchored };
}

/** Plain text glyphs, no colour: the weight comes from CSS. */
export function severityGlyph(s: Severity): string {
  switch (s) {
    case 'high':
      return '!!';
    case 'medium':
      return '!';
    case 'low':
      return '·';
    case 'info':
      return '–';
  }
}

export function severityLabel(s: Severity): string {
  switch (s) {
    case 'high':
      return 'High';
    case 'medium':
      return 'Medium';
    case 'low':
      return 'Low';
    case 'info':
      return 'Note';
  }
}

/** The message sent to the original agent for a finding or a free comment. */
export function formatFollowup(file: string, line: number | null, text: string): string {
  return line === null ? `${file} — ${text}` : `${file}:${line} — ${text}`;
}
