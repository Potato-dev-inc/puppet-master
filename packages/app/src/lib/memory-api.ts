// Typed wrappers for the project-memory commands (`commands/one_brain.rs`).
import { invoke } from '@tauri-apps/api/core';

export type MemoryTargetStatus = 'in_sync' | 'will_create' | 'will_add' | 'will_update' | 'will_remove' | 'nothing' | 'blocked';

export interface MemoryTarget {
  path: string;
  agents: string[];
  status: MemoryTargetStatus;
  /** The file has text of its own outside the managed block. */
  user_content: boolean;
}

export interface MemoryView {
  project: string;
  memory: string;
  global_enabled: boolean;
  project_enabled: boolean;
  targets: MemoryTarget[];
  report: { written: string[]; removed: string[]; errors: string[] } | null;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const memoryApi = {
  get: (project?: string) => call<MemoryView>('memory_get', { project }),
  save: (text: string, project?: string) => call<MemoryView>('memory_save', { project, text }),
  sync: (project?: string) => call<MemoryView>('memory_sync', { project }),
  setProjectEnabled: (enabled: boolean, project?: string) =>
    call<MemoryView>('memory_set_project_enabled', { project, enabled }),
  globalEnabled: () => call<boolean>('memory_global_enabled'),
  setGlobalEnabled: (enabled: boolean) => call<boolean>('memory_set_global_enabled', { enabled }),
};

export type MemoryApi = typeof memoryApi;
