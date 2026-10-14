// The Tauri commands Simple mode adds (`commands/plain_mode.rs`), typed. The UI never says where a
// task runs: there is no argument for it. Rust always makes a copy.
import { invoke } from '@tauri-apps/api/core';

export interface StartInput {
  name: string;
  prompt: string;
  agent: string;
  /** A folder she already has. Leave out to make a new project. */
  existing_folder?: string;
}

export interface Started {
  pane_id: string | null;
  handle: string;
  folder: string;
}

export type FolderCheck = { ok: true } | { ok: false; code: string; message: string };

export interface PreviewFile {
  name: string;
  /** `null`: the page is too big to show here. */
  html: string | null;
}

export interface SimpleApi {
  checkFolder: (path: string) => Promise<FolderCheck>;
  start: (input: StartInput) => Promise<Started>;
  /** The next step on a project's run: a message, or the task again after the trust question. */
  send: (project: string, handle: string, text: string) => Promise<{ status: string; pane_id: string | null }>;
  preview: (paneId: string) => Promise<PreviewFile | null>;
  openPreview: (paneId: string) => Promise<void>;
  pickFolder: () => Promise<{ path: string; name: string } | null>;
}

/** The code at the front of a Rust error ("FOLDER_TOO_BROAD: ..."), or `null`. */
export function errorCode(err: unknown): string | null {
  const text = err instanceof Error ? err.message : String(err);
  const match = /^([A-Z][A-Z_]+):/.exec(text);
  return match ? match[1] : null;
}

export const simpleApi: SimpleApi = {
  checkFolder: (path) => invoke<FolderCheck>('plain_check_folder', { path }),
  // Every field is a fact about what she wants. Nothing here can switch the copy off.
  start: (input) => invoke<Started>('plain_start', { input }),
  send: (project, handle, text) => invoke<{ status: string; pane_id: string | null }>('plain_send', { project, handle, text }),
  preview: (paneId) => invoke<PreviewFile | null>('plain_preview', { paneId }),
  openPreview: (paneId) => invoke<void>('plain_open_preview', { paneId }),
  pickFolder: () => invoke<{ path: string; name: string } | null>('project_pick_folder'),
};
