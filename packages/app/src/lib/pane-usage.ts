// Usage per helper pane: the turns and busy time of each pane's run, and the tokens its chat reported
// (only when it did). Shape mirrors the Rust `budgets::usage_of_pane` (src-tauri/src/budgets/pane_usage.rs).
// Turns come from the operation records; tokens from the chat's own usage item, never from the screen.
import { invoke } from '@tauri-apps/api/core';

export interface UsageTokens {
  input: number;
  output: number;
}

export interface HelperUsage {
  paneId: string;
  /** Set only when the pane runs under a known account. */
  accountLabel?: string | null;
  turns: number;
  busyMs: number;
  /** Set only when the pane's chat reported usage. */
  tokens?: UsageTokens | null;
}

export interface PaneUsage {
  own: { turns: number; busyMs: number };
  /** Every pane under this one, nested helpers included. */
  helperCount: number;
  helperTurns: number;
  helperBusyMs: number;
  /** The direct helpers. */
  helpers: HelperUsage[];
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** The usage of a pane and its helpers; `null` outside the desktop shell. */
export async function paneUsage(paneId: string): Promise<PaneUsage | null> {
  if (!inTauri()) return null;
  return invoke<PaneUsage>('pane_usage', { paneId });
}
