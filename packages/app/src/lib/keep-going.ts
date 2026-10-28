// Keep going: when an agent runs out of usage, the person can move the task to another agent.
//
// The Rust side (src-tauri/src/handoff/keep_going and commands/keep_going.rs) holds everything; this
// file is the typed command surface, the view shapes and the event name. Field names mirror the serde
// names exactly: `Target` and `Settings` are snake_case, the views and `Offer` are camelCase. Tauri
// maps the Rust argument names to camelCase (`pane_id` is `paneId`).
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** `agent` or `agent@account` (Rust `Target`). `account` is missing when there is none. */
export interface KeepGoingTarget {
  agent_id: string;
  account?: string;
}

/** One picker row from `keep_going_targets` (Rust `Offer`). `whyNot` says why `ok` is false. */
export interface KeepGoingOffer {
  target: KeepGoingTarget;
  label: string;
  ok: boolean;
  whyNot: string | null;
}

export type KeepGoingState = 'stopping' | 'starting' | 'running' | 'failed';
export type KeepGoingReason = 'usage_limit' | 'came_back';

/** One hand-over as the windows see it (Rust `HandoverView`). No brief, folder or goal text. */
export interface HandoverView {
  id: string;
  fromLabel: string;
  toLabel: string;
  fromPaneId: string;
  toPaneId: string;
  atMs: number;
  state: KeepGoingState;
  stateText: string;
  turns: number;
  briefBytes: number;
  /** When the move-back card is due, while it still waits for the reset. */
  backAtMs: number | null;
  reason: KeepGoingReason;
}

/** Both sides of one pane's hand-overs (Rust `Lineage`). `from` = "Continued from", `to` = "Continued on". */
export interface KeepGoingLineage {
  from: HandoverView | null;
  to: HandoverView | null;
}

/** Settings > Agents > Keep going (Rust `Settings`). */
export interface KeepGoingSettings {
  order: KeepGoingTarget[];
  ask_back: boolean;
}

/** Payload of the `keep-going://changed` event. */
export interface KeepGoingChanged {
  paneIds: string[];
}

export const KEEP_GOING_CHANGED_EVENT = 'keep-going://changed';

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** `agent` or `agent@account`, the same text the Rust `Target::key` gives. */
export function keepGoingTargetKey(target: KeepGoingTarget): string {
  return target.account ? `${target.agent_id}@${target.account}` : target.agent_id;
}

/** The picker rows for a pane. Rejects for a pane that cannot hand over. */
export function keepGoingTargets(paneId: string): Promise<KeepGoingOffer[]> {
  return invoke<KeepGoingOffer[]>('keep_going_targets', { paneId });
}

/** Start the hand-over the person chose. `target` is the `agent` or `agent@account` text. */
export function keepGoingContinue(
  paneId: string,
  target: string,
  back: boolean,
  cardId?: string | null,
): Promise<HandoverView> {
  return invoke<HandoverView>('keep_going_continue', { paneId, target, back, cardId: cardId ?? null });
}

/** Both sides of a pane's hand-overs. Outside the desktop window there are none. */
export function keepGoingLineage(paneId: string): Promise<KeepGoingLineage> {
  if (!inTauri()) return Promise.resolve({ from: null, to: null });
  return invoke<KeepGoingLineage>('keep_going_lineage', { paneId });
}

/** The stored brief of one hand-over (already redacted). Desktop window only. */
export function keepGoingBrief(id: string): Promise<string> {
  return invoke<string>('keep_going_brief', { id });
}

export function keepGoingSettingsGet(): Promise<KeepGoingSettings> {
  return invoke<KeepGoingSettings>('keep_going_settings_get');
}

export function keepGoingSettingsSet(settings: KeepGoingSettings): Promise<KeepGoingSettings> {
  return invoke<KeepGoingSettings>('keep_going_settings_set', { settings });
}

/** Calls `cb` when hand-over records change. Outside the desktop window nothing is listened to. */
export function onKeepGoingChanged(cb: (event: KeepGoingChanged) => void): Promise<UnlistenFn> {
  if (!inTauri()) return Promise.resolve(() => {});
  return listen<KeepGoingChanged>(KEEP_GOING_CHANGED_EVENT, (event) => cb(event.payload));
}

/**
 * The line a hand-over shows in plain words. The server text wins; the fallback only covers an
 * empty string so the banner never goes blank.
 */
export function stateText(view: HandoverView): string {
  const text = view.stateText.trim();
  if (text) return text;
  if (view.state === 'running') return `Continued on ${view.toLabel}`;
  if (view.state === 'failed') return `${view.toLabel} couldn't start`;
  return `Moving to ${view.toLabel}`;
}

/**
 * The line one pane shows. The server sends one `HandoverView` for both panes, so a running hand-over reads
 * "Continued on {to}" on the old pane and "Continued from {from}" on the new one. Every other state (moving,
 * failed) keeps the server text through `stateText`.
 */
export function sideText(view: HandoverView, isNewPane: boolean): string {
  if (view.state !== 'running') return stateText(view);
  return isNewPane ? `Continued from ${view.fromLabel}` : `Continued on ${view.toLabel}`;
}
