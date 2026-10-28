// "Open this chat at that message": the search (or anything else) asks for a jump, the pane view opens its Chat
// tab, and the chat scrolls to the message once it is loaded. One jump waits at a time; whoever handles it clears it.
import { useSyncExternalStore } from 'react';
import type { ChatTurn } from './sessionChatModel';

export interface ChatJump {
  paneId: string;
  itemId: string;
  /** Tells two jumps to the same message apart. */
  nonce: number;
}

let pending: ChatJump | null = null;
let nonce = 0;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of [...listeners]) listener();
}

export function requestChatJump(paneId: string, itemId: string): void {
  pending = { paneId, itemId, nonce: ++nonce };
  emit();
}

/** The jump for this pane, while one waits. */
export function useChatJump(paneId: string): ChatJump | null {
  const jump = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => pending,
  );
  return jump && jump.paneId === paneId ? jump : null;
}

/** The jump was handled (or can not be): stop offering it. Another jump that came meanwhile stays. */
export function clearChatJump(jump: ChatJump): void {
  if (pending?.nonce !== jump.nonce) return;
  pending = null;
  emit();
}

/** The turn that holds the item (the person's message, the agent's text, or a tool call), or null. */
export function turnWithItem(turns: ChatTurn[], itemId: string): ChatTurn | null {
  for (const turn of turns) {
    if (turn.user?.id === itemId) return turn;
    for (const step of turn.steps) {
      const id = step.kind === 'tool' ? step.call.id : step.item.id;
      if (id === itemId) return turn;
    }
  }
  return null;
}
