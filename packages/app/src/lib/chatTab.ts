// "Show this pane on its Chat tab": whoever opens a pane for its chat (an old chat continued in a new pane) asks here,
// and the pane view switches to its Chat tab once it is shown. One request waits at a time; the pane view clears it.
import { useSyncExternalStore } from 'react';

export interface ChatTabRequest {
  paneId: string;
  /** Tells two requests for the same pane apart. */
  nonce: number;
}

let pending: ChatTabRequest | null = null;
let nonce = 0;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of [...listeners]) listener();
}

export function requestChatTab(paneId: string): void {
  pending = { paneId, nonce: ++nonce };
  emit();
}

/** The request for this pane, while one waits. */
export function useChatTabRequest(paneId: string): ChatTabRequest | null {
  const request = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => pending,
  );
  return request && request.paneId === paneId ? request : null;
}

/** The request was handled: stop offering it. A newer request that came meanwhile stays. */
export function clearChatTabRequest(request: ChatTabRequest): void {
  if (pending?.nonce !== request.nonce) return;
  pending = null;
  emit();
}
