// Search across all chats (Ctrl+Shift+F). The Rust side (src-tauri/src/chat_index) keeps a redacted index of what
// was said in every chat that was read; this file is its typed command and the small helpers that draw a hit.
import { invoke } from '@tauri-apps/api/core';

export type ChatHitKind = 'user' | 'assistant' | 'tool';

/** One result. `marks`: `[start, end)` ranges of `snippet` to highlight, in UTF-16 units (a JS string's own). */
export interface ChatHit {
  paneId: string;
  session: string;
  itemId: string;
  kind: ChatHitKind;
  /** The agent's id; `agentLabel` turns it into the name the person knows. */
  agent: string;
  /** The name of the folder (not the path). */
  folder: string;
  atMs: number;
  snippet: string;
  marks: [number, number][];
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const CHAT_SEARCH_LIMIT = 30;

/** `enabled` is false when the person turned the search index off: then there are no hits. */
export interface ChatSearchAnswer {
  enabled: boolean;
  hits: ChatHit[];
}

export async function searchChats(query: string, limit = CHAT_SEARCH_LIMIT): Promise<ChatSearchAnswer> {
  if (!isTauri()) return { enabled: true, hits: [] };
  return invoke<ChatSearchAnswer>('chat_search', { query, limit });
}

export interface SnippetPart {
  text: string;
  hit: boolean;
}

/** The snippet cut into plain and highlighted parts. Ranges that are out of order or past the end are ignored. */
export function snippetParts(snippet: string, marks: [number, number][]): SnippetPart[] {
  const parts: SnippetPart[] = [];
  let at = 0;
  for (const [start, end] of marks) {
    if (start < at || end <= start || end > snippet.length) continue;
    if (start > at) parts.push({ text: snippet.slice(at, start), hit: false });
    parts.push({ text: snippet.slice(start, end), hit: true });
    at = end;
  }
  if (at < snippet.length) parts.push({ text: snippet.slice(at), hit: false });
  return parts;
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** When a hit happened, short: "5 min ago", "3 h ago", "Yesterday", "12 Oct", "12 Oct 2025". */
export function whenText(atMs: number, now: number): string {
  const diff = now - atMs;
  if (diff < 60_000) return 'Just now';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} min ago`;
  const at = new Date(atMs);
  const today = new Date(now);
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOfDay(today) - startOfDay(at)) / 86_400_000);
  if (days <= 0) return `${Math.floor(diff / 3_600_000)} h ago`;
  if (days === 1) return 'Yesterday';
  const base = `${at.getDate()} ${MONTHS[at.getMonth()]}`;
  return at.getFullYear() === today.getFullYear() ? base : `${base} ${at.getFullYear()}`;
}

/** Who spoke, in a word. */
export function kindText(kind: ChatHitKind): string {
  return kind === 'user' ? 'You' : kind === 'tool' ? 'Tool' : 'Agent';
}
