// The search index of your chats (Settings > General): what it holds, its on/off switch and clearing it. The Rust
// side is src-tauri/src/commands/chat_index_settings.rs; the search itself is `chatSearch.ts`.
import { invoke } from '@tauri-apps/api/core';

/** `items`: chat items kept; `bytes`: the size of the file; `enabled`: the switch the person set; `oldCopies`: damaged-index copies still on disk. */
export interface ChatIndexStatus {
  enabled: boolean;
  items: number;
  bytes: number;
  oldCopies?: number;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export async function chatIndexStatus(): Promise<ChatIndexStatus> {
  if (!isTauri()) return { enabled: true, items: 0, bytes: 0, oldCopies: 0 };
  return invoke<ChatIndexStatus>('chat_index_status');
}

/** Turns the index on or off and saves it. Off: no chat is copied and nothing is searched. */
export async function chatIndexSetEnabled(enabled: boolean): Promise<ChatIndexStatus> {
  return invoke<ChatIndexStatus>('chat_index_set_enabled', { enabled });
}

/** Removes the saved search text. Chats themselves are not touched. */
export async function chatIndexClear(): Promise<ChatIndexStatus> {
  return invoke<ChatIndexStatus>('chat_index_clear');
}

/** Bytes in one short figure: "812 B", "96 KB", "2.1 MB". */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1024) return `${Math.max(0, Math.round(bytes || 0))} B`;
  const units = ['KB', 'MB', 'GB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

/** "2.1 MB · 4,312 items", or "1 item". */
export function indexSizeText(status: Pick<ChatIndexStatus, 'items' | 'bytes'>): string {
  const count = status.items.toLocaleString('en-US');
  return `${formatBytes(status.bytes)} · ${count} ${status.items === 1 ? 'item' : 'items'}`;
}

/** The line under "Clear search index": the size, and the old copies of a damaged index when there are any. */
export function indexStatusText(status: Pick<ChatIndexStatus, 'items' | 'bytes' | 'oldCopies'>): string {
  const old = status.oldCopies ?? 0;
  if (old <= 0) return indexSizeText(status);
  return `${indexSizeText(status)} · ${old} old damaged ${old === 1 ? 'copy' : 'copies'} (removed too)`;
}
