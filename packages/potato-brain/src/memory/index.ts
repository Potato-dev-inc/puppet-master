import type { DocInput, Hit } from "./types.ts";
import { tokenize } from "./tokenize.ts";

const K1 = 1.2;
const B = 0.75;
const RECENCY_MIN = 0.95;
const DEFAULT_LIMIT = 8;
const MAX_WHY = 4;

interface Unit {
  key: string;
  path: string;
  line: number;
  mtime: number;
  length: number;
  counts: Map<string, number>;
}

interface SerializedUnit {
  line: number;
  length: number;
  counts: Record<string, number>;
}

interface SerializedDoc {
  path: string;
  mtime: number;
  units: SerializedUnit[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function comparePath(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function ratioOf(value: number, lo: number, span: number): number {
  if (span <= 0) return 1;
  return Math.min(1, Math.max(0, (value - lo) / span));
}

export class MemoryIndex {
  private readonly docs = new Map<string, number>();
  private readonly units = new Map<string, Unit>();
  private readonly byPath = new Map<string, string[]>();
  private readonly postings = new Map<string, Set<string>>();
  private totalLength = 0;

  add(doc: DocInput): void {
    this.remove(doc.path);
    this.docs.set(doc.path, doc.mtime);
    this.byPath.set(doc.path, []);

    const lines = doc.text.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const raw = lines[i];
      if (raw === undefined) continue;
      const text = raw.trim();
      if (text.length === 0) continue;
      const tokens = tokenize(text);
      const counts = new Map<string, number>();
      for (const token of tokens) counts.set(token, (counts.get(token) ?? 0) + 1);
      this.insertUnit(doc.path, i + 1, doc.mtime, tokens.length, counts);
    }
  }

  /**
   * Index a single appended line without re-reading the whole file. The
   * journal grows on every message, so a full re-scan per line is the one
   * thing that would not scale.
   */
  appendLine(path: string, line: number, text: string, mtime: number): void {
    const trimmed = text.trim();
    if (trimmed === "") return;

    const key = `${path}:${line}`;
    this.dropUnit(key);

    const tokens = tokenize(trimmed);
    if (tokens.length === 0) return;

    const counts = new Map<string, number>();
    for (const token of tokens) counts.set(token, (counts.get(token) ?? 0) + 1);

    this.docs.set(path, mtime);
    this.insertUnit(path, line, mtime, tokens.length, counts);
  }

  private dropUnit(key: string): void {
    const unit = this.units.get(key);
    if (unit === undefined) return;

    this.totalLength -= unit.length;
    for (const token of unit.counts.keys()) {
      const posting = this.postings.get(token);
      if (posting === undefined) continue;
      posting.delete(key);
      if (posting.size === 0) this.postings.delete(token);
    }
    this.units.delete(key);

    const keys = this.byPath.get(unit.path);
    if (keys) {
      const at = keys.indexOf(key);
      if (at !== -1) keys.splice(at, 1);
    }
  }

  remove(path: string): void {    const keys = this.byPath.get(path);
    if (keys === undefined) return;
    for (const key of keys) {
      const unit = this.units.get(key);
      if (unit === undefined) continue;
      this.totalLength -= unit.length;
      for (const token of unit.counts.keys()) {
        const posting = this.postings.get(token);
        if (posting === undefined) continue;
        posting.delete(key);
        if (posting.size === 0) this.postings.delete(token);
      }
      this.units.delete(key);
    }
    this.byPath.delete(path);
    this.docs.delete(path);
  }

  search(query: string, limit: number = DEFAULT_LIMIT): Hit[] {
    const queryTokens = tokenize(query);
    if (queryTokens.length === 0) return [];

    const unitCount = this.units.size;
    if (unitCount === 0) return [];

    const wanted = Math.floor(limit);
    if (!(wanted > 0)) return [];

    const avgLength = this.totalLength > 0 ? this.totalLength / unitCount : 1;

    let newest = -Infinity;
    let oldest = Infinity;
    for (const unit of this.units.values()) {
      if (unit.mtime > newest) newest = unit.mtime;
      if (unit.mtime < oldest) oldest = unit.mtime;
    }
    const span = newest - oldest;

    const hits: Hit[] = [];
    for (const unit of this.units.values()) {
      let score = 0;
      const matched: string[] = [];

      for (const token of queryTokens) {
        const tf = unit.counts.get(token);
        if (tf === undefined || tf <= 0) continue;
        const df = this.postings.get(token)?.size ?? 0;
        if (df === 0) continue;
        const idf = Math.log(1 + (unitCount - df + 0.5) / (df + 0.5));
        const norm = 1 - B + (B * unit.length) / avgLength;
        score += idf * ((tf * (K1 + 1)) / (tf + K1 * norm));
        matched.push(token);
      }

      if (score <= 0) continue;

      const recency = RECENCY_MIN + (1 - RECENCY_MIN) * ratioOf(unit.mtime, oldest, span);
      hits.push({
        path: unit.path,
        line: unit.line,
        score: score * recency,
        why: `matched: ${matched.slice(0, MAX_WHY).join(" ")}`,
      });
    }

    hits.sort((a, b) => b.score - a.score || comparePath(a.path, b.path) || a.line - b.line);
    return hits.slice(0, wanted);
  }

  get docCount(): number {
    return this.docs.size;
  }

  toJSON(): unknown {
    const docs: SerializedDoc[] = [];
    for (const [path, mtime] of this.docs) {
      const keys = this.byPath.get(path) ?? [];
      const sorted = [...keys].sort(
        (a, b) => (this.units.get(a)?.line ?? 0) - (this.units.get(b)?.line ?? 0),
      );
      const units: SerializedUnit[] = [];
      for (const key of sorted) {
        const unit = this.units.get(key);
        if (unit === undefined) continue;
        units.push({
          line: unit.line,
          length: unit.length,
          counts: Object.fromEntries(unit.counts),
        });
      }
      docs.push({ path, mtime, units });
    }
    return { version: 1, docs };
  }

  static fromJSON(value: unknown): MemoryIndex {
    const index = new MemoryIndex();
    if (!isRecord(value)) return index;
    const rawDocs = value["docs"];
    if (!Array.isArray(rawDocs)) return index;

    for (const rawDoc of rawDocs) {
      if (!isRecord(rawDoc)) continue;
      const path = rawDoc["path"];
      if (typeof path !== "string") continue;
      const mtimeRaw = rawDoc["mtime"];
      const mtime = typeof mtimeRaw === "number" && Number.isFinite(mtimeRaw) ? mtimeRaw : 0;

      if (!index.docs.has(path)) {
        index.docs.set(path, mtime);
        index.byPath.set(path, []);
      }

      const rawUnits = rawDoc["units"];
      if (!Array.isArray(rawUnits)) continue;
      for (const rawUnit of rawUnits) {
        if (!isRecord(rawUnit)) continue;
        const lineRaw = rawUnit["line"];
        const line = typeof lineRaw === "number" ? lineRaw : 0;

        const counts = new Map<string, number>();
        let sum = 0;
        const rawCounts = rawUnit["counts"];
        if (isRecord(rawCounts)) {
          for (const [token, count] of Object.entries(rawCounts)) {
            if (typeof count !== "number" || !Number.isFinite(count) || count <= 0) continue;
            counts.set(token, count);
            sum += count;
          }
        }

        const lengthRaw = rawUnit["length"];
        const length =
          typeof lengthRaw === "number" && Number.isFinite(lengthRaw) && lengthRaw >= 0
            ? lengthRaw
            : sum;

        if (index.units.has(`${path}:${line}`)) continue;
        index.insertUnit(path, line, mtime, length, counts);
      }
    }

    return index;
  }

  private insertUnit(
    path: string,
    line: number,
    mtime: number,
    length: number,
    counts: Map<string, number>,
  ): void {
    const key = `${path}:${line}`;
    const unit: Unit = { key, path, line, mtime, length, counts };
    this.units.set(key, unit);

    const keys = this.byPath.get(path);
    if (keys === undefined) this.byPath.set(path, [key]);
    else keys.push(key);

    for (const token of counts.keys()) {
      const posting = this.postings.get(token);
      if (posting === undefined) this.postings.set(token, new Set([key]));
      else posting.add(key);
    }

    this.totalLength += length;
  }
}
