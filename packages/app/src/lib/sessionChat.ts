// Read-only chat view of a terminal pane's agent session file. The Rust side (src-tauri/src/session_chat)
// reads the CLI's own session file and emits the items; this file is the typed Tauri command surface. The item
// types and the pure merge/grouping helpers live in sessionChatModel.ts (no Tauri import, shared with the phone)
// and are re-exported here, so desktop imports keep working.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { SessionChatItemsEvent, SessionChatSnapshot, SessionChatSourceEvent, SessionChatStatusEvent } from './sessionChatModel';

export * from './sessionChatModel';

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

// A pane's open and close reach the backend one after the other, in the order they were asked: the app's
// dev build mounts, unmounts and mounts the chat again at once, and a close that overtook the second open
// would leave the pane with a view and no reader.
const queues = new Map<string, Promise<unknown>>();

function inOrder<T>(paneId: string, run: () => Promise<T>): Promise<T> {
  const done = (queues.get(paneId) ?? Promise.resolve()).then(run, run);
  const tail = done.catch(() => undefined);
  queues.set(paneId, tail);
  void tail.then(() => {
    if (queues.get(paneId) === tail) queues.delete(paneId);
  });
  return done;
}

/**
 * Start reading this pane's session file. The backend keeps tailing it until every surface that opened it has
 * called `closeSessionChat`. `surface` is this view's own id: two views of one pane (the cockpit and a pane view)
 * each hold the reader, so closing one does not stop the other's.
 */
export function openSessionChat(paneId: string, surface?: string): Promise<SessionChatSnapshot> {
  return inOrder(paneId, () => call<SessionChatSnapshot>('session_chat_open', { paneId, surface }));
}

export function closeSessionChat(paneId: string, surface?: string): Promise<void> {
  return inOrder(paneId, () => call<void>('session_chat_close', { paneId, surface }));
}

/** Every pane's item batches; the caller filters by `paneId`. Resolves to a no-op outside the desktop shell. */
export function onSessionChatItems(cb: (event: SessionChatItemsEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(noopUnlisten);
  return listen<SessionChatItemsEvent>('session-chat-items', (e) => cb(e.payload));
}

/** What an empty chat waits for, per pane; the caller filters by `paneId`. A no-op outside the desktop shell. */
export function onSessionChatStatus(cb: (event: SessionChatStatusEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(noopUnlisten);
  return listen<SessionChatStatusEvent>('session-chat-status', (e) => cb(e.payload));
}

/** Where a pane's chat is read from changed (an API chat falls back to the terminal and back). No-op outside the desktop shell. */
export function onSessionChatSource(cb: (event: SessionChatSourceEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(noopUnlisten);
  return listen<SessionChatSourceEvent>('session-chat-source', (e) => cb(e.payload));
}
