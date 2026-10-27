// The Chat tab's message box: messages a person wrote for the agent in a pane. The Rust side
// (src-tauri/src/pane_outbox) keeps them in a short list per pane, types the first one when the agent is ready
// and tells the windows about every change with the `pane-outbox` event. This file is the typed command surface
// and the pure rules the view uses; nothing else.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type OutboxState = 'queued' | 'sending' | 'sent' | 'failed' | 'cancelled';

/** One message, as the backend sends it (camelCase). `createdAt` is milliseconds since the epoch. */
export interface OutboxMessage {
  id: string;
  text: string;
  createdAt: number;
  state: OutboxState;
  /** While queued: one plain sentence saying what it waits for. */
  waitingFor: string | null;
  /** When failed: one plain sentence. */
  error: string | null;
}

/** Payload of the `pane-outbox` event: the pane's whole list. */
export interface PaneOutboxEvent {
  paneId: string;
  messages: OutboxMessage[];
}

/** The longest message (characters). The backend refuses more. */
export const MAX_MESSAGE_CHARS = 8000;
/** How long a sent message stays as a bubble if the real message has not shown up in the chat yet. */
export const SENT_BUBBLE_MS = 10_000;

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

async function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

/** Adds a message to the pane's list. Rejects with one plain sentence when the backend refuses it. */
export function sendPaneMessage(paneId: string, text: string): Promise<OutboxMessage> {
  return call<OutboxMessage>('pane_message_send', { paneId, text });
}

/** Cancels a message that has not started to be typed. */
export function cancelPaneMessage(paneId: string, messageId: string): Promise<OutboxMessage> {
  return call<OutboxMessage>('pane_message_cancel', { paneId, messageId });
}

export async function listPaneMessages(paneId: string): Promise<OutboxMessage[]> {
  const list = await call<OutboxMessage[] | null>('pane_messages_list', { paneId });
  return Array.isArray(list) ? list : [];
}

export function onPaneOutbox(cb: (event: PaneOutboxEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(noopUnlisten);
  return listen<PaneOutboxEvent>('pane-outbox', (e) => cb({ paneId: e.payload.paneId, messages: Array.isArray(e.payload.messages) ? e.payload.messages : [] }));
}

/** The words of an error the backend or the bridge threw (they are already plain sentences). */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return 'Could not send the message.';
}

/** Why the message box is switched off, or null when the person can write. Plain words. */
export function composerBlock(pane: { status: string } | undefined, isShell: boolean): string | null {
  if (isShell) return 'Chat replies need an agent pane.';
  if (!pane) return 'This pane is not open any more.';
  if (pane.status === 'error') return 'The agent is not running. Open Terminal.';
  return null;
}

const norm = (text: string): string => text.replace(/\s+/g, ' ').trim();

/**
 * The messages to show as bubbles under the conversation, oldest first: waiting and sending ones, failed ones
 * (until the person hides them), and a sent one until the real message shows up in the chat (a user message
 * with the same words) or `SENT_BUBBLE_MS` after it was first seen as sent. Cancelled ones are not shown.
 * `sentSeen`: when each message was first seen as sent (ms); `hidden`: ids the person hid.
 */
export function bubblesFor(
  messages: readonly OutboxMessage[],
  chatUserTexts: readonly string[],
  sentSeen: ReadonlyMap<string, number>,
  hidden: ReadonlySet<string>,
  now: number,
): OutboxMessage[] {
  const said = new Set(chatUserTexts.map(norm));
  return messages.filter((m) => {
    if (hidden.has(m.id)) return false;
    switch (m.state) {
      case 'queued':
      case 'sending':
      case 'failed':
        return true;
      case 'sent': {
        if (said.has(norm(m.text))) return false;
        const seen = sentSeen.get(m.id);
        return seen === undefined || now - seen < SENT_BUBBLE_MS;
      }
      default:
        return false;
    }
  });
}

/**
 * `list` with a message the person just sent added. The answer of `pane_message_send` can come after the event
 * that already carries a newer state of the same message, so a message the list knows is left as it is.
 */
export function withSent(list: readonly OutboxMessage[], sent: OutboxMessage): OutboxMessage[] {
  if (list.some((m) => m.id === sent.id)) return [...list];
  return [...list, sent].sort((a, b) => a.createdAt - b.createdAt);
}

/**
 * Where a message box gets its messages. The desktop asks the Tauri commands; a phone asks the PC's bridge
 * (`paneOutbox-bridge.ts`). ChatComposer, PendingMessages and usePaneOutbox only ever talk to this shape.
 */
export interface OutboxTransport {
  send(paneId: string, text: string): Promise<OutboxMessage>;
  cancel(paneId: string, messageId: string): Promise<OutboxMessage>;
  list(paneId: string): Promise<OutboxMessage[]>;
  /** Calls `cb` with the pane's whole list on every change. Returns the stop function. */
  watch(paneId: string, cb: (messages: OutboxMessage[]) => void): () => void;
}

/** The desktop implementation: the Tauri commands and the `pane-outbox` event. */
export const tauriOutboxTransport: OutboxTransport = {
  send: sendPaneMessage,
  cancel: cancelPaneMessage,
  list: listPaneMessages,
  watch(paneId, cb) {
    let live = true;
    let unlisten: UnlistenFn | null = null;
    void onPaneOutbox((event) => {
      if (live && event.paneId === paneId) cb(event.messages);
    })
      .then((off) => {
        if (live) unlisten = off;
        else off();
      })
      .catch(() => undefined);
    return () => {
      live = false;
      unlisten?.();
    };
  },
};
