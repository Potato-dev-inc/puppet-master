// "Pick for me": which agent to start in this folder, from your own history. Shape mirrors the Rust
// `scores::pick::model` types (src-tauri/src/scores/pick/model.rs, camelCase). Desktop only: no bridge route,
// no MCP tool. The call sends the folder path and nothing else. There is no task text, and nothing is stored,
// logged or sent. The answer is one plain sentence and counts, never a verdict.
import { invoke } from '@tauri-apps/api/core';
import type { ScoreSample } from './agent-scores';

/** Why the pick was made. `none_ready` = no agent can run. */
export type PickBasis = 'clear' | 'only_one' | 'too_early' | 'close_call' | 'none_ready';

/** The measure that decided, or was compared. */
export type PickMetric = 'checks_green' | 'merged' | 'ended_ok';

/** The agent the menu starts. `accountId` is set only for a saved login (the same id the Scores page returns). */
export interface PickChoice {
  agentId: string;
  accountId?: string;
  label: string;
}

/** One line of "Why this one": a ready agent or a skipped one. */
export type PickConsideredState = 'ready' | 'skipped';

export interface PickConsidered {
  label: string;
  state: PickConsideredState;
  line: string;
}

/** The whole answer of `pick_agent`. `choice` is null when nothing is ready. */
export interface PickResult {
  choice: PickChoice | null;
  /** Exactly one sentence. */
  reason: string;
  basis: PickBasis;
  metric: PickMetric | null;
  /** The sample of the deciding rate. `early` = 5 to 9 runs. */
  sample: ScoreSample;
  /** The chosen one first, then ready, then skipped. */
  considered: PickConsidered[];
  /** Plain sentences, shown under "Why this one". */
  notes: string[];
}

export interface PickApi {
  /** `project` is the folder path the menu already shows. Nothing else is sent. */
  pick: (project: string) => Promise<PickResult>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const tauriPickApi: PickApi = {
  pick: async (project) => {
    if (!isTauri()) throw new Error('Desktop shell required: pick_agent');
    return invoke<PickResult>('pick_agent', { project });
  },
};

/** The state of the "Pick for me" row: the call is still running, or it has answered. */
export type PickState = PickResult | 'loading';

/** The muted hint on the row: "Looking...", the label, or "None ready". */
export function pickHint(state: PickState): string {
  if (state === 'loading') return 'Looking...';
  return state.choice ? state.choice.label : 'None ready';
}

/** True only when the deciding numbers come from a few runs (5 to 9). The page shows the `early` chip. */
export function isEarly(result: PickResult): boolean {
  return result.sample === 'early';
}

/** The primary button text, "Start Fake A". Plain "Start" when nothing is ready. */
export function startLabel(result: PickResult): string {
  return result.choice ? `Start ${result.choice.label}` : 'Start';
}

/**
 * Can the menu start the chosen agent? It must be a choice, and an entry of the menu with the same type
 * must exist and must not be disabled. Otherwise the panel shows the reason and only Back.
 */
export function startable(result: PickResult, entries: { type: string; disabled?: boolean }[]): boolean {
  const choice = result.choice;
  if (!choice) return false;
  return entries.some((entry) => entry.type === choice.agentId && !entry.disabled);
}

/** The title of the `early` chip. */
export const EARLY_TITLE = 'Based on only a few runs. It may change.';

/** The footer of "Why this one". */
export const FOOTER = 'Worked out on your computer. Nothing you type is sent anywhere.';
