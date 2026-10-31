import { appendFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import type { Dirent } from "node:fs";
import { dirname, isAbsolute, join, resolve, sep } from "node:path";

export interface Jot {
  path: string;
  line: number;
}

const DEFAULT_SUBDIRS = ["journal", "diary", "nodes", "sessions"];

function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

export function dayKey(date: Date): string {
  const y = date.getFullYear();
  const m = pad2(date.getMonth() + 1);
  const d = pad2(date.getDate());
  return `${y}-${m}-${d}`;
}

export function journalPath(root: string, date: Date): string {
  const key = dayKey(date);
  return join(resolve(root), "journal", key.slice(0, 4), key.slice(5, 7), `${key}.md`);
}

function resolveInside(root: string, relPath: string): string {
  const rel = relPath.replace(/\\/g, "/");
  if (relPath === "" || isAbsolute(relPath) || isAbsolute(rel) || /^[A-Za-z]:/.test(relPath)) {
    throw new Error(`path escapes vault: ${relPath}`);
  }
  for (const part of rel.split("/")) {
    if (part === "..") throw new Error(`path escapes vault: ${relPath}`);
  }
  const base = resolve(root);
  const full = resolve(base, rel);
  if (full !== base && !full.startsWith(base + sep)) {
    throw new Error(`path escapes vault: ${relPath}`);
  }
  return full;
}

function lineCount(text: string): number {
  let n = 0;
  for (const ch of text) if (ch === "\n") n++;
  return n;
}

function splitLines(text: string): string[] {
  const normalized = text.replace(/\r\n?/g, "\n");
  const trimmed = normalized.endsWith("\n") ? normalized.slice(0, -1) : normalized;
  return trimmed === "" ? [] : trimmed.split("\n");
}

export function appendJournal(root: string, date: Date, entry: string): Jot {
  const key = dayKey(date);
  const full = journalPath(root, date);
  const dir = dirname(full);
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });

  let before = existsSync(full) ? readFileSync(full, "utf8") : "";
  if (before === "") {
    before = `# ${key}\n\n`;
    writeFileSync(full, before, "utf8");
  }

  const text = entry.replace(/\r\n?/g, "\n").split("\n").join(" ").trim();
  const stamp = `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  const separator = before.endsWith("\n") ? "" : "\n";
  const line = lineCount(before) + 1 + (separator === "" ? 0 : 1);

  appendFileSync(full, `${separator}- ${stamp} ${text}\n`, "utf8");

  return {
    path: ["journal", key.slice(0, 4), key.slice(5, 7), `${key}.md`].join("/"),
    line,
  };
}

export function listMarkdown(root: string, subdirs: string[] = DEFAULT_SUBDIRS): string[] {
  const out: string[] = [];
  for (const sub of subdirs) {
    let start: string;
    let prefix: string[];
    try {
      start = resolveInside(root, sub);
      prefix = sub.replace(/\\/g, "/").split("/").filter((p) => p !== "" && p !== ".");
    } catch {
      continue;
    }
    walk(start, prefix, out);
  }
  out.sort();
  return out;
}

function walk(dir: string, prefix: string[], out: string[]): void {
  let entries: Dirent[];
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  for (const entry of entries) {
    if (entry.name.startsWith(".") || entry.isSymbolicLink()) continue;
    if (entry.isDirectory()) {
      walk(join(dir, entry.name), [...prefix, entry.name], out);
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".md")) {
      out.push([...prefix, entry.name].join("/"));
    }
  }
}

export function readWindow(root: string, relPath: string, fromLine: number, count: number): string {
  const lines = splitLines(readDoc(root, relPath));
  if (lines.length === 0 || !Number.isFinite(count) || count <= 0) return "";
  const want = Math.floor(Number.isFinite(fromLine) ? fromLine : 1);
  const start = Math.max(1, want);
  if (start > lines.length) return "";
  const end = Math.min(lines.length, start + Math.floor(count) - 1);
  return lines.slice(start - 1, end).join("\n");
}

export function readDoc(root: string, relPath: string): string {
  const full = resolveInside(root, relPath);
  return readFileSync(full, "utf8").replace(/\r\n?/g, "\n");
}

export function appendLine(root: string, relPath: string, line: string): void {
  const full = resolveInside(root, relPath);
  mkdirSync(dirname(full), { recursive: true });
  appendFileSync(full, line.endsWith("\n") ? line : `${line}\n`, "utf8");
}
