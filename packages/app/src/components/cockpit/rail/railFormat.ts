// Pure text helpers for the session rail's cards. No React.
import type { OutputFile, UsageItem } from '../../../lib/sessionChatModel';

/** "48k", "1.2M", "850": a token count in a few characters. */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '0';
  if (n < 1000) return String(Math.round(n));
  if (n < 1_000_000) return `${n < 10_000 ? (n / 1000).toFixed(1).replace(/\.0$/, '') : Math.round(n / 1000)}k`;
  return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
}

export interface ContextReading {
  used: number;
  window: number;
  /** 0 to 100, whole number. */
  percent: number;
  input: number;
  output: number;
}

/** What the agent reported, or null when it did not say how big the window is (never a guess). */
export function readContext(usage: UsageItem | null): ContextReading | null {
  if (!usage || !usage.contextWindow || usage.contextWindow <= 0) return null;
  const used = usage.inputTokens + usage.outputTokens;
  const percent = Math.min(100, Math.round((used / usage.contextWindow) * 100));
  return { used, window: usage.contextWindow, percent, input: usage.inputTokens, output: usage.outputTokens };
}

const slashes = (path: string) => path.replace(/\\/g, '/');

/** The last part of a path: "src/app.ts" gives "app.ts". */
export function baseName(path: string): string {
  const parts = slashes(path).split('/').filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** The last two parts of a folder path, for a narrow line: "C:/work/shop/app" gives "shop/app". */
export function shortFolder(path: string): string {
  const parts = slashes(path).split('/').filter(Boolean);
  if (parts.length === 0) return path;
  return parts.slice(-2).join('/');
}

/** A touched file's folder, relative to the pane's folder when it sits inside it ("" for the folder itself). */
export function fileFolder(path: string, cwd: string): string {
  const file = slashes(path);
  const root = slashes(cwd).replace(/\/+$/, '');
  const dir = file.slice(0, Math.max(0, file.lastIndexOf('/')));
  if (root && dir.toLowerCase() === root.toLowerCase()) return '';
  if (root && dir.toLowerCase().startsWith(`${root.toLowerCase()}/`)) return dir.slice(root.length + 1);
  return shortFolder(dir);
}

/** Files the agent changed (an edit or a write), not the ones it only read. */
export function writtenFiles(outputs: readonly OutputFile[]): OutputFile[] {
  return outputs.filter((o) => o.wrote);
}

/** "+1 file", "+3 files". */
export function filesCount(n: number): string {
  return `+${n} ${n === 1 ? 'file' : 'files'}`;
}
