// Typed wrappers for the Point and Fix commands (`commands/point.rs`) and events (`point://state`, `point://draft`).
// The Rust side owns the picks, the shots and the send rules; this file only names them and mirrors their shapes (camelCase).
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { OutboxMessage } from './paneOutbox';

/** One picked element, as the Point box shows it. */
export interface PickView {
  id: string;
  /** "localhost:5173/pricing" */
  page: string;
  title: string;
  /** "button.card-cta" */
  label: string;
  text: string;
  component?: string | null;
  source?: { relPath: string; line?: number | null } | null;
  /** A data: PNG, only when small enough to send to the box. */
  thumb?: string | null;
  /** Path relative to the target pane folder, e.g. `.puppet-master/shots/...`. */
  shotPath?: string | null;
  /** Why there is no screenshot (one plain sentence). */
  noShotReason?: string | null;
}

/** A pane the message can go to. */
export interface Target {
  paneId: string;
  agentLabel: string;
  title: string;
  sameFolder: boolean;
  /** False for panes the outbox refuses; `reason` says why. */
  canSend: boolean;
  reason?: string | null;
}

export interface PointState {
  picks: PickView[];
  targets: Target[];
  defaultTarget?: string | null;
  /** One plain line, e.g. a blocked link. */
  notice?: string | null;
  /** The live session's id. It changes when a new session starts; null when no session is live. */
  session?: number | null;
}

/** What `point_open_preview` returns. */
export interface PreviewInfo {
  previewId: string;
  url: string;
  title: string;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const pointApi = {
  /** Opens the app preview for a pane's dev server. `armed` also starts the picker. */
  async openPreview(paneId: string, port: number, armed = false): Promise<PreviewInfo> {
    const info = await call<PreviewInfo>('point_open_preview', { paneId, port });
    if (armed) await call<void>('point_arm');
    return info;
  },
  state: () => call<PointState>('point_state'),
  arm: () => call<void>('point_arm'),
  removePick: (id: string) => call<PointState>('point_remove_pick', { pickId: id }),
  /** Builds the message text for a target, without sending it (used by Copy). */
  compose: (target: string, request: string) => call<string>('point_compose', { targetPaneId: target, request }),
  send: (target: string, request: string) => call<OutboxMessage>('point_send', { targetPaneId: target, request }),
  /** Puts the message into the pane's Chat box as a draft and ends the session. */
  editInChat: (target: string, request: string) => call<void>('point_edit_in_chat', { targetPaneId: target, request }),
  takeDraft: (paneId: string) => call<string | null>('point_take_draft', { paneId }),
  /** Deletes the session's own screenshots, hides the box and disarms the picker. */
  discard: () => call<void>('point_discard'),
  hide: () => call<void>('point_box_hide'),
  openBlockedLink: () => call<void>('point_open_blocked_link'),
  onState(cb: (s: PointState) => void): () => void {
    if (!isTauri()) return () => {};
    let off: UnlistenFn | undefined;
    let disposed = false;
    listen<PointState>('point://state', (e) => cb(e.payload))
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  },
  onDraft(cb: (paneId: string) => void): () => void {
    if (!isTauri()) return () => {};
    let off: UnlistenFn | undefined;
    let disposed = false;
    listen<{ paneId: string }>('point://draft', (e) => cb(e.payload.paneId))
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  },
};
