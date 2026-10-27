// "Remember for all agents": the text selected in a pane's Chat tab becomes one line of the project memory
// (`one_brain_remember` / `one_brain_forget` in `commands/one_brain.rs`). The backend names the pane; it finds
// the project folder itself.
import { invoke } from '@tauri-apps/api/core';
import type { MemoryView } from './memory-api';

export interface RememberResult {
  /** The line as stored (cleaned, capped). */
  text: string;
  /** False when the memory already had that line. */
  added: boolean;
  view: MemoryView;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const rememberApi = {
  remember: (paneId: string, text: string) => call<RememberResult>('one_brain_remember', { paneId, text }),
  forget: (paneId: string, text: string) => call<MemoryView>('one_brain_forget', { paneId, text }),
  setProjectEnabled: (paneId: string, enabled: boolean) => call<MemoryView>('one_brain_set_project_enabled', { paneId, enabled }),
};

export type RememberApi = typeof rememberApi;

/** The backend's code for "the memory is off for this project" (the Turn on offer). */
export const MEMORY_OFF_PROJECT = 'MEMORY_OFF_PROJECT';

/** The stable code the backend puts before the colon of an error (`SECRET: ...`), or '' when there is none. */
export function rememberErrorCode(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  const match = /^([A-Z_]+):/.exec(text);
  return match ? match[1] : '';
}

/** What the person reads for a failed remember: the backend's sentence, without its code. */
export function rememberErrorMessage(err: unknown): string {
  const text = err instanceof Error ? err.message : String(err);
  const cut = text.indexOf(': ');
  return /^[A-Z_]+:/.test(text) && cut > 0 ? text.slice(cut + 2) : text;
}
