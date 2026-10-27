// The phone's message box transport: the same OutboxTransport the desktop has, over the PC's bridge. Sends and
// cancels are the `/panes/{id}/messages` routes. Changes arrive as the `pane-outbox` SSE event (PwaApp hands it to
// `publishBridgeOutbox`); while the box is on screen the list is also read every few seconds, as a fallback.
import type { BridgeClient } from './bridge-types';
import type { OutboxMessage, OutboxTransport } from './paneOutbox';

/** How often the phone reads the list while the box is on screen and the page is visible. */
export const OUTBOX_POLL_MS = 3000;

const REACH_FAILED = 'Could not reach your PC.';
const OLD_PC = 'This PC is too old to take messages from the phone.';
const SEND_FAILED = 'Could not send the message.';
const CANCEL_FAILED = 'Could not cancel the message.';

type Watcher = (messages: OutboxMessage[]) => void;
const watchers = new Map<string, Set<Watcher>>();

/** A `pane-outbox` event came in from the PC: tell the boxes that watch this pane. */
export function publishBridgeOutbox(paneId: string, messages: OutboxMessage[]): void {
  for (const notify of [...(watchers.get(paneId) ?? [])]) notify(messages);
}

/**
 * The plain sentence of a refused bridge call. The bridge error reads `bridge POST /p -> 400: {"error":"..."}`;
 * the person only sees the `error` sentence. A network failure (no status) says the PC could not be reached.
 */
export function bridgeRefusalText(error: unknown, fallback: string): string {
  const text = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  const at = text.match(/ -> \d{3}: ([\s\S]*)$/);
  if (!at) return REACH_FAILED;
  const body = at[1]!.trim();
  try {
    const parsed = JSON.parse(body) as { error?: unknown };
    if (typeof parsed.error === 'string' && parsed.error) return parsed.error;
  } catch {
    if (body && !body.startsWith('{')) return body;
  }
  return fallback;
}

export function bridgeOutboxTransport(bridge: BridgeClient): OutboxTransport {
  const list = async (paneId: string): Promise<OutboxMessage[]> => (bridge.listPaneMessages ? bridge.listPaneMessages(paneId) : []);

  return {
    async send(paneId, text) {
      if (!bridge.sendPaneMessage) throw new Error(OLD_PC);
      try {
        return await bridge.sendPaneMessage(paneId, text);
      } catch (error) {
        throw new Error(bridgeRefusalText(error, SEND_FAILED));
      }
    },
    async cancel(paneId, messageId) {
      if (!bridge.cancelPaneMessage) throw new Error(OLD_PC);
      try {
        return await bridge.cancelPaneMessage(paneId, messageId);
      } catch (error) {
        throw new Error(bridgeRefusalText(error, CANCEL_FAILED));
      }
    },
    list,
    watch(paneId, cb) {
      let live = true;
      const notify: Watcher = (messages) => {
        if (live) cb(messages);
      };
      const set = watchers.get(paneId) ?? new Set<Watcher>();
      set.add(notify);
      watchers.set(paneId, set);
      const timer = setInterval(() => {
        if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
        list(paneId)
          .then(notify)
          .catch(() => undefined);
      }, OUTBOX_POLL_MS);
      return () => {
        live = false;
        clearInterval(timer);
        const current = watchers.get(paneId);
        current?.delete(notify);
        if (current && current.size === 0) watchers.delete(paneId);
      };
    },
  };
}
