/** One indexable unit: a markdown file in the vault. */
export interface DocInput {
  /** vault-relative path, e.g. "journal/2026/10/2026-10-28.md" */
  path: string;
  text: string;
  /** epoch ms, used only for recency tie-breaking */
  mtime: number;
}

/** A pointer into the vault. Retrieval returns these, never prose. */
export interface Hit {
  path: string;
  /** 1-based line number inside the file */
  line: number;
  score: number;
  /** short human-readable reason, e.g. "matched: 白飯 吃飯" */
  why: string;
}

export interface Pointers {
  hits: Hit[];
  /** tokens actually spent on this result, for the budget log */
  tokens: number;
}
