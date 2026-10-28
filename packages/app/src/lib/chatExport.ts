// "Save chat as page" and "Save as Markdown": the Rust side (commands/chat_export.rs) makes one self-contained HTML
// page or one Markdown file of a pane's chat; these are the typed command wrappers. Desktop only; nothing is uploaded.
import { invoke } from '@tauri-apps/api/core';

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

/** What a saved chat can be: one HTML page, or one Markdown file. */
export type ExportFormat = 'page' | 'markdown';

/** The page Rust made (the preview shows `html`) and the one-time token that saves exactly that page. */
export interface ExportPage {
  html: string;
  token: string;
}

/** The Markdown file Rust made (the preview shows `markdown` as text) and the one-time token that saves exactly that file. */
export interface ExportMarkdown {
  markdown: string;
  token: string;
}

/** The whole chat of a pane as one HTML document, with the token that `chatExportSave` needs. */
export function chatExportHtml(paneId: string): Promise<ExportPage> {
  return call<ExportPage>('chat_export_html', { paneId });
}

/** The whole chat of a pane as one Markdown file, with the token that `chatExportSave` needs. */
export function chatExportMarkdown(paneId: string): Promise<ExportMarkdown> {
  return call<ExportMarkdown>('chat_export_markdown', { paneId });
}

/** Writes the page or file behind `token` to `path` (what the save dialog returned). The window never sends the text itself. Resolves to the path written. */
export function chatExportSave(token: string, path: string): Promise<string> {
  return call<string>('chat_export_save', { token, path });
}

/** A default file name for the save dialog, e.g. `chat-2026-10-28-1530.html` or `.md` for Markdown. */
export function chatPageFileName(now: Date = new Date(), format: ExportFormat = 'page'): string {
  const p = (n: number) => String(n).padStart(2, '0');
  const ext = format === 'markdown' ? 'md' : 'html';
  return `chat-${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}.${ext}`;
}
