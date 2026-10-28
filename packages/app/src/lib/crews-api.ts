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

/** The agent a starter role has until the crew starts: the person's default agent. */
export const DEFAULT_AGENT = '{default}';

export interface CrewAgent {
  id: string;
  label: string;
}

export interface CrewAgents {
  /** Agents that can take a role here (installed and able to take a task). */
  agents: CrewAgent[];
  /** The agent `{default}` stands for; null when none is set up. */
  default: string | null;
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
  /** The crews the person saved. */
  list: () => Promise<CrewSummary[]>;
  /** The built-in starter crews. They are never saved unless copied. */
  starters: () => Promise<CrewSummary[]>;
  /** A saved crew or a starter, by slug. */
  get: (slug: string) => Promise<Crew>;
  agents: () => Promise<CrewAgents>;
  draft: (paneId: string) => Promise<CrewDraft>;
  /** `replace` overwrites the saved crew with the same name (the sheet asks first). */
  save: (crew: Crew, replace?: boolean) => Promise<CrewSaveOutcome>;
  remove: (slug: string) => Promise<void>;
  /** `agents` maps role names to the agent the Start sheet chose for them. */
  start: (slug: string, folder: string, agents?: Record<string, string>) => Promise<CrewStartOutcome>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const crewsApi: CrewsApi = {
  list: () => call<CrewSummary[]>('crews_list'),
  starters: () => call<CrewSummary[]>('crews_starters'),
  get: (slug) => call<Crew>('crews_get', { slug }),
  agents: () => call<CrewAgents>('crews_agents'),
  draft: (paneId) => call<CrewDraft>('crews_draft', { paneId }),
  save: (crew, replace = false) => call<CrewSaveOutcome>('crews_save', { crew, replace }),
  remove: (slug) => call<void>('crews_remove', { slug }),
  start: (slug, folder, agents) => call<CrewStartOutcome>('crews_start', { slug, folder, agents }),
};

/** A command error is `CODE: message`; the message is the part to show. */
export function crewErrorText(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  return raw.replace(/^[A-Z][A-Z_]+: /, '');
}
