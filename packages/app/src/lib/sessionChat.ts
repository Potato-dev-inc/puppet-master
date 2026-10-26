// Read-only chat view of a terminal pane's agent session file. The Rust side (src-tauri/src/session_chat)
// reads the CLI's own session file and emits the items below; this file is the typed command surface,
// the pure merge/grouping helpers and nothing else. Agent text and tool output are data: the view shows
// them as plain text only.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type TurnOutcome = 'completed' | 'interrupted' | 'failed';

/** One parsed line of the session file. Mirrors `ChatItem` in src-tauri/src/session_chat (camelCase). */
export type ChatItem =
  | { kind: 'user'; id: string; at: string | null; text: string }
  | { kind: 'assistant'; id: string; at: string | null; text: string; model: string | null; finalAnswer: boolean }
  | { kind: 'toolCall'; id: string; at: string | null; callId: string; name: string; summary: string }
  | { kind: 'toolResult'; id: string; at: string | null; callId: string; ok: boolean; output: string }
  | { kind: 'turnEnd'; id: string; at: string | null; outcome: TurnOutcome; durationMs: number | null }
  | {
      kind: 'usage';
      id: string;
      at: string | null;
      inputTokens: number;
      outputTokens: number;
      contextWindow: number | null;
    }
  | { kind: 'session'; id: string; at: string | null; model: string | null; cliVersion: string | null };

export type UserItem = Extract<ChatItem, { kind: 'user' }>;
export type AssistantItem = Extract<ChatItem, { kind: 'assistant' }>;
export type ToolCallItem = Extract<ChatItem, { kind: 'toolCall' }>;
export type ToolResultItem = Extract<ChatItem, { kind: 'toolResult' }>;
export type TurnEndItem = Extract<ChatItem, { kind: 'turnEnd' }>;
export type UsageItem = Extract<ChatItem, { kind: 'usage' }>;

/** `session_chat_open` answer: whether this pane has a session file we can read, and the items so far. */
export interface SessionChatSnapshot {
  available: boolean;
  reason: string | null;
  items: ChatItem[];
}

/** Payload of the `session-chat-items` event. `reset`: the file was replaced or truncated, drop old items. */
export interface SessionChatItemsEvent {
  paneId: string;
  items: ChatItem[];
  reset: boolean;
}

export type ToolStatus = 'running' | 'ok' | 'error';

export interface ToolStep {
  kind: 'tool';
  id: string;
  call: ToolCallItem;
  /** Null until the result arrives: the tool is still running. */
  result: ToolResultItem | null;
  status: ToolStatus;
}

export interface TextStep {
  kind: 'text';
  id: string;
  item: AssistantItem;
}

export type TurnStep = TextStep | ToolStep;

/** One exchange: what the person typed, then the agent's steps, then how the turn ended (if it did). */
export interface ChatTurn {
  id: string;
  /** Null for agent output before any person's message (a session opened mid-turn). */
  user: UserItem | null;
  steps: TurnStep[];
  end: TurnEndItem | null;
  outcome: TurnOutcome | null;
  durationMs: number | null;
}

export interface GroupedChat {
  turns: ChatTurn[];
  /** Latest known model and CLI version (the newest `session` item that has each). */
  model: string | null;
  cliVersion: string | null;
  /** Latest `usage` item. */
  usage: UsageItem | null;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

/** Start reading this pane's session file. The backend keeps tailing it until `closeSessionChat`. */
export function openSessionChat(paneId: string): Promise<SessionChatSnapshot> {
  return call<SessionChatSnapshot>('session_chat_open', { paneId });
}

export function closeSessionChat(paneId: string): Promise<void> {
  return call<void>('session_chat_close', { paneId });
}

/** Every pane's item batches; the caller filters by `paneId`. Resolves to a no-op outside the desktop shell. */
export function onSessionChatItems(cb: (event: SessionChatItemsEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(noopUnlisten);
  return listen<SessionChatItemsEvent>('session-chat-items', (e) => cb(e.payload));
}

/**
 * Add `incoming` to `prev`, one entry per `id`. An item whose `id` is already there replaces it in place,
 * so the order is the order of first arrival. `reset` drops `prev` first (the file was replaced).
 */
export function mergeItems(prev: readonly ChatItem[], incoming: readonly ChatItem[], reset: boolean): ChatItem[] {
  const byId = new Map<string, ChatItem>();
  if (!reset) for (const item of prev) byId.set(item.id, item);
  for (const item of incoming) byId.set(item.id, item);
  return [...byId.values()];
}

/**
 * Group items into turns. A turn starts at a `user` item and ends at its `turnEnd`. A tool result attaches
 * to the call with the same `callId` (a call without one is still running). Agent output before any user
 * item starts a turn with no `user`.
 */
export function groupTurns(items: readonly ChatItem[]): GroupedChat {
  const turns: ChatTurn[] = [];
  const calls = new Map<string, ToolStep>();
  let current: ChatTurn | null = null;
  let model: string | null = null;
  let cliVersion: string | null = null;
  let usage: UsageItem | null = null;

  const turnFor = (id: string): ChatTurn => {
    if (current) return current;
    current = { id, user: null, steps: [], end: null, outcome: null, durationMs: null };
    turns.push(current);
    return current;
  };

  for (const item of items) {
    switch (item.kind) {
      case 'user':
        current = { id: item.id, user: item, steps: [], end: null, outcome: null, durationMs: null };
        turns.push(current);
        break;
      case 'assistant':
        turnFor(item.id).steps.push({ kind: 'text', id: item.id, item });
        break;
      case 'toolCall': {
        const step: ToolStep = { kind: 'tool', id: item.id, call: item, result: null, status: 'running' };
        calls.set(item.callId, step);
        turnFor(item.id).steps.push(step);
        break;
      }
      case 'toolResult': {
        const step = calls.get(item.callId);
        if (step) {
          step.result = item;
          step.status = item.ok ? 'ok' : 'error';
        }
        break;
      }
      case 'turnEnd': {
        const turn = turnFor(item.id);
        turn.end = item;
        turn.outcome = item.outcome;
        turn.durationMs = item.durationMs;
        current = null;
        break;
      }
      case 'usage':
        usage = item;
        break;
      case 'session':
        model = item.model ?? model;
        cliVersion = item.cliVersion ?? cliVersion;
        break;
    }
  }
  return { turns, model, cliVersion, usage };
}
