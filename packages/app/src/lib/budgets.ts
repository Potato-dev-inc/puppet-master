// Budgets: how much an agent run may do before it must ask you (turns and minutes). The header chip
// and its popover read `budget_for_pane`; "Add 10 turns" and "Stop here" use `budget_adjust`.
// Shape mirrors the Rust `budgets::view` (src-tauri/src/budgets/view.rs). Off by default: a pane
// whose run has no budget reads `null` and shows nothing.
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type BudgetState = 'ok' | 'nearly' | 'used_up' | 'stopped';

export interface BudgetHelper {
  /** Agent name ("Codex"). */
  agent: string;
  used: number;
  limit: number | null;
  closed: boolean;
}

export interface BudgetView {
  state: BudgetState;
  depth: number;
  is_helper: boolean;
  turns: { used: number; own: number; held_for_helpers: number; limit: number; add_step: number | null } | null;
  minutes: { used: number; limit: number; add_step: number | null } | null;
  /** A turn is still running and the time is up: it pauses after this turn. */
  time_up_in_turn: boolean;
  helpers: BudgetHelper[];
}

export type BudgetAction = 'add_turns' | 'add_minutes' | 'stop';

export interface BudgetApi {
  forPane: (paneId: string) => Promise<BudgetView | null>;
  /** Resolves with the sentence to show ("Added 10 turns."). */
  adjust: (paneId: string, action: BudgetAction) => Promise<string>;
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const tauriBudgetApi: BudgetApi = {
  forPane: async (paneId) => (inTauri() ? invoke<BudgetView | null>('budget_for_pane', { paneId }) : null),
  adjust: (paneId, action) => invoke<string>('budget_adjust', { paneId, action }),
};

const plural = (n: number, unit: string): string => `${n} ${unit}${n === 1 ? '' : 's'}`;

/** The chip: "12 of 20 turns", "14 of 60 min", or what happened ("Budget used up"). */
export function chipText(view: BudgetView): string {
  if (view.state === 'stopped') return 'Stopped at its budget';
  if (view.time_up_in_turn) return "Time's up · pauses after this turn";
  if (view.state === 'used_up') return 'Budget used up';
  if (view.turns) return `${view.turns.used} of ${plural(view.turns.limit, 'turn')}`;
  if (view.minutes) return `${view.minutes.used} of ${view.minutes.limit} min`;
  return 'Budget';
}

/** "Add 10 turns" / "Add 30 min": the step the Inbox card offers too. */
export const addTurnsLabel = (view: BudgetView): string | null =>
  view.turns?.add_step ? `Add ${plural(view.turns.add_step, 'turn')}` : null;
export const addMinutesLabel = (view: BudgetView): string | null =>
  view.minutes?.add_step ? `Add ${view.minutes.add_step} min` : null;

/** "Codex · 4 of 10 turns (from this budget)". */
export function helperLine(helper: BudgetHelper): string {
  const share = helper.limit === null ? plural(helper.used, 'turn') : `${helper.used} of ${plural(helper.limit, 'turn')}`;
  return `${helper.agent} · ${share}${helper.closed ? ', finished' : ''} (from this budget)`;
}

/** The pane's budget, refreshed when `refreshKey` changes (a turn ended, a status changed) and
 *  every few seconds while shown. `null` while loading and when the pane has no budget. */
export function useBudget(paneId: string, refreshKey: unknown, api: BudgetApi = tauriBudgetApi) {
  const [view, setView] = useState<BudgetView | null>(null);
  const refresh = useCallback(() => {
    void api.forPane(paneId).then(setView).catch(() => setView(null));
  }, [api, paneId]);
  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, 8000);
    return () => clearInterval(timer);
  }, [refresh, refreshKey]);
  return { view, refresh };
}
