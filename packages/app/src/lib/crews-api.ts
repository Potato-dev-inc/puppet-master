// Typed wrappers for the crew commands (`commands/crews.rs`): a crew is a saved team of agents.
// "Save this team" reads a draft from a live pane; "Start crew" starts a saved one in a folder.
import { invoke } from '@tauri-apps/api/core';

export interface CrewRole {
  name: string;
  agent_type: string;
  /** May use `{folder}`. */
  task: string;
  /** The role this one is under; null or missing for the lead. */
  parent?: string | null;
}

export interface Crew {
  name: string;
  roles: CrewRole[];
}

export interface CrewDraft {
  crew: Crew;
  /** Helpers left out because a crew holds at most 8 roles. */
  omitted: number;
}

export interface CrewSummary {
  slug: string;
  name: string;
  roles: number;
  agents: string[];
  /** Why this crew cannot be started right now (a broken file, an agent that is gone). */
  problem: string | null;
}

export interface CrewProblem {
  code: string;
  role: string | null;
  message: string;
}

export interface CrewSaveOutcome {
  slug: string | null;
  errors: CrewProblem[];
}

export interface CrewStartOutcome {
  started: { role: string; agent_type: string; pane_id: string }[];
  failed: { role: string; code: string; message: string } | null;
  /** Roles not tried because an earlier one failed. */
  not_started: string[];
}

export interface CrewsApi {
  list: () => Promise<CrewSummary[]>;
  draft: (paneId: string) => Promise<CrewDraft>;
  /** `replace` overwrites the saved crew with the same name (the sheet asks first). */
  save: (crew: Crew, replace?: boolean) => Promise<CrewSaveOutcome>;
  remove: (slug: string) => Promise<void>;
  start: (slug: string, folder: string) => Promise<CrewStartOutcome>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const crewsApi: CrewsApi = {
  list: () => call<CrewSummary[]>('crews_list'),
  draft: (paneId) => call<CrewDraft>('crews_draft', { paneId }),
  save: (crew, replace = false) => call<CrewSaveOutcome>('crews_save', { crew, replace }),
  remove: (slug) => call<void>('crews_remove', { slug }),
  start: (slug, folder) => call<CrewStartOutcome>('crews_start', { slug, folder }),
};

/** A command error is `CODE: message`; the message is the part to show. */
export function crewErrorText(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  return raw.replace(/^[A-Z][A-Z_]+: /, '');
}
