// "Bring my old chats": the types, the typed wrappers for the `chat_import_*` commands, the progress listener, the
// small line helpers, and the two request stores (open an old chat, open the Bring panel). The Rust side is
// src-tauri/src/commands/chat_import.rs. Nothing here reads an agent folder: that starts only after "Find my chats".
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useSyncExternalStore } from 'react';
import { formatBytes, type ChatIndexStatus } from './chatIndex';
import type { ChatItem } from './sessionChatModel';

/** One agent in the list. `fromDefinition`: it comes from a TOML agent definition, so it starts off. */
export interface ImportAgent {
  id: string;
  label: string;
  fromDefinition: boolean;
  enabled: boolean;
}

export type ImportPhase = 'idle' | 'looking' | 'reading' | 'done' | 'cancelled' | 'failed';

/** What one agent's store holds, from a quick look (no chat is read). Times are ms; null when unknown. */
export interface ImportFound {
  id: string;
  label: string;
  fromDefinition: boolean;
  chats: number;
  bytes: number;
  oldestMs: number | null;
  newestMs: number | null;
  already: number;
  tooBig: number;
  places: string[];
  canContinue: boolean;
}

/** A run in progress. `agent` is the agent id. Counters for skipped and too-big chats show only when above 0. */
export interface ImportProgress {
  phase: ImportPhase;
  agent: string | null;
  chatsDone: number;
  chatsTotal: number;
  items: number;
  unchanged: number;
  tooBig: number;
  unreadable: number;
  message: string | null;
}

/** `imported`: how many old chats and search rows are in the index now. */
export interface ImportState {
  asked: boolean;
  sinceDays: number | null;
  indexEnabled: boolean;
  agents: ImportAgent[];
  progress: ImportProgress;
  imported: { chats: number; items: number };
}

/** The index status, plus the counts of old chats in it. */
export type ImportStatus = ChatIndexStatus & { importedChats?: number; importedItems?: number };

export interface ImportedChatMeta {
  title: string;
  agent: string;
  folder: string;
  updatedMs: number;
  cutShort: boolean;
}

export interface OpenedImportedChat {
  meta: ImportedChatMeta;
  items: ChatItem[];
}

/** How to start the agent on an old chat. `ok = false` gives a plain `reason`; nothing is typed into the pane. */
export interface ResumePlan {
  ok: boolean;
  reason?: string;
  agentType: string;
  cwd: string;
  extraArgs: string[];
}

/** The calls the Bring panel makes. Tests inject a fake one; the app uses `chatImportApi`. */
export interface ImportApi {
  state: () => Promise<ImportState>;
  look: (sinceDays: number | null) => Promise<ImportFound[]>;
  start: (agents: string[], sinceDays: number | null) => Promise<ImportProgress>;
  cancel: () => Promise<ImportProgress>;
  dismiss: () => Promise<void>;
  clear: () => Promise<ImportStatus>;
  open: (agent: string, session: string) => Promise<OpenedImportedChat>;
  resumePlan: (agent: string, session: string) => Promise<ResumePlan>;
}

export const CHAT_IMPORT_PROGRESS_EVENT = 'chat-import-progress';
export const DEFAULT_SINCE_DAYS = 365;

/** The "Only chats from the last ..." choices. `days: null` is all chats. */
export const SINCE_CHOICES: { label: string; days: number | null }[] = [
  { label: '30 days', days: 30 },
  { label: '90 days', days: 90 },
  { label: '12 months', days: DEFAULT_SINCE_DAYS },
  { label: 'All', days: null },
];

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function idleProgress(): ImportProgress {
  return { phase: 'idle', agent: null, chatsDone: 0, chatsTotal: 0, items: 0, unchanged: 0, tooBig: 0, unreadable: 0, message: null };
}

/** Outside the desktop app there is nothing to import: `asked` is true so the first-start card stays hidden. */
function notHere(): ImportState {
  return { asked: true, sinceDays: DEFAULT_SINCE_DAYS, indexEnabled: true, agents: [], progress: idleProgress(), imported: { chats: 0, items: 0 } };
}

/** The settings and the progress so far. Does not read any agent folder. */
export async function chatImportState(): Promise<ImportState> {
  if (!isTauri()) return notHere();
  return invoke<ImportState>('chat_import_state');
}

/** Lists what each agent holds. Only after the person pressed "Find my chats". */
export async function chatImportLook(sinceDays: number | null): Promise<ImportFound[]> {
  if (!isTauri()) return [];
  const answer = await invoke<{ found: ImportFound[] }>('chat_import_look', { sinceDays });
  return answer.found;
}

/** Starts reading the chosen agents' chats. Also saves the choice and marks the first question as answered. */
export async function chatImportStart(agents: string[], sinceDays: number | null): Promise<ImportProgress> {
  if (!isTauri()) return idleProgress();
  return invoke<ImportProgress>('chat_import_start', { agents, sinceDays });
}

/** Stops a run. Chats read so far stay. */
export async function chatImportCancel(): Promise<ImportProgress> {
  if (!isTauri()) return idleProgress();
  return invoke<ImportProgress>('chat_import_cancel');
}

/** "Not now": marks the question as answered. Nothing is read. */
export async function chatImportDismiss(): Promise<void> {
  if (!isTauri()) return;
  await invoke<unknown>('chat_import_dismiss');
}

/** "Remove imported chats": removes the old chats from search. The agents' own files are not touched. */
export async function chatImportClear(): Promise<ImportStatus> {
  if (!isTauri()) return { enabled: true, items: 0, bytes: 0, oldCopies: 0, importedChats: 0, importedItems: 0 };
  return invoke<ImportStatus>('chat_import_clear');
}

/** One old chat, read-only, for the viewer. */
export async function chatImportOpen(agent: string, session: string): Promise<OpenedImportedChat> {
  if (!isTauri()) throw new Error('Old chats can only be opened in the desktop app.');
  return invoke<OpenedImportedChat>('chat_import_open', { agent, session });
}

/** The plan to continue an old chat in a new pane. */
export async function chatImportResumePlan(agent: string, session: string): Promise<ResumePlan> {
  if (!isTauri()) {
    return { ok: false, reason: 'Continuing a chat works in the desktop app only.', agentType: agent, cwd: '', extraArgs: [] };
  }
  return invoke<ResumePlan>('chat_import_resume_plan', { agent, session });
}

/** The real wrappers, in the shape the panels take (see `ImportApi`). */
export const chatImportApi: ImportApi = {
  state: chatImportState,
  look: chatImportLook,
  start: chatImportStart,
  cancel: chatImportCancel,
  dismiss: chatImportDismiss,
  clear: chatImportClear,
  open: chatImportOpen,
  resumePlan: chatImportResumePlan,
};

/** Listens for `chat-import-progress` (sent at most 4 times a second). Returns the stop function. */
export function onImportProgress(callback: (progress: ImportProgress) => void): () => void {
  if (!isTauri()) return () => {};
  let closed = false;
  let stop: (() => void) | null = null;
  void listen<ImportProgress>(CHAT_IMPORT_PROGRESS_EVENT, (event) => callback(event.payload)).then((unlisten) => {
    if (closed) unlisten();
    else stop = unlisten;
  });
  return () => {
    closed = true;
    stop?.();
  };
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const WEEK_MS = 7 * 86_400_000;

function monthYear(ms: number): string {
  const at = new Date(ms);
  return `${MONTHS[at.getMonth()]} ${at.getFullYear()}`;
}

function rangeText(oldestMs: number | null, newestMs: number | null, now: number): string | null {
  if (oldestMs === null || newestMs === null) return null;
  const end = now - newestMs < WEEK_MS ? 'now' : monthYear(newestMs);
  const start = monthYear(oldestMs);
  return end === start ? start : `${start} to ${end}`;
}

/** A count with its noun: "1 chat", "412 chats". Every count on the Bring my old chats screens uses it. */
export function chatCount(count: number): string {
  return `${count.toLocaleString('en-US')} ${count === 1 ? 'chat' : 'chats'}`;
}

/** One row of the agent list: "Claude Code · 412 chats · 38 MB · Mar 2026 to now". */
export function foundLine(found: ImportFound, now: number = Date.now()): string {
  const chats = chatCount(found.chats);
  const parts = [found.label, chats, formatBytes(found.bytes)];
  const range = rangeText(found.oldestMs, found.newestMs, now);
  if (range) parts.push(range);
  return parts.join(' · ');
}

/** The line under the progress bar. `label` is the agent's name (the id is used when it is missing). */
export function progressLine(progress: ImportProgress, label?: string): string {
  const name = label ?? progress.agent ?? '';
  switch (progress.phase) {
    case 'looking':
      return 'Looking…';
    case 'reading': {
      const what = name ? `${name} chats` : 'chats';
      return `Reading ${what}… ${progress.chatsDone} of ${progress.chatsTotal}`;
    }
    case 'done':
      return 'Done.';
    case 'cancelled':
      return 'Stopped. The chats read so far are kept.';
    case 'failed':
      return progress.message ?? 'Could not read the old chats.';
    default:
      return '';
  }
}

/** The key of an old chat in the viewer: `import:<agent>:<session>`. */
export function importedChatKey(agent: string, session: string): string {
  return `import:${agent}:${session}`;
}

/** The request to open an old chat at a message (from a search hit). */
export interface ImportedChatRequest {
  agent: string;
  session: string;
  itemId?: string;
  nonce: number;
}

/** The request to open the Bring my old chats panel. */
export interface BringChatsRequest {
  nonce: number;
}

let lastNonce = 0;

/** One pending request at a time, like `chatJump.ts`. The handler clears it; a newer request is not cleared by an old one. */
function makeRequestStore<T extends { nonce: number }>() {
  let pending: T | null = null;
  const listeners = new Set<() => void>();
  const emit = () => {
    for (const listener of [...listeners]) listener();
  };
  return {
    request(value: Omit<T, 'nonce'>): void {
      pending = { ...value, nonce: ++lastNonce } as T;
      emit();
    },
    use(): T | null {
      return useSyncExternalStore(
        (listener) => {
          listeners.add(listener);
          return () => listeners.delete(listener);
        },
        () => pending,
      );
    },
    clear(request: T): void {
      if (pending?.nonce !== request.nonce) return;
      pending = null;
      emit();
    },
  };
}

const importedChatStore = makeRequestStore<ImportedChatRequest>();
const bringChatsStore = makeRequestStore<BringChatsRequest>();

export function requestImportedChat(agent: string, session: string, itemId?: string): void {
  importedChatStore.request({ agent, session, itemId });
}

export function useImportedChatRequest(): ImportedChatRequest | null {
  return importedChatStore.use();
}

export function clearImportedChatRequest(request: ImportedChatRequest): void {
  importedChatStore.clear(request);
}

export function requestBringChats(): void {
  bringChatsStore.request({});
}

export function useBringChatsRequest(): BringChatsRequest | null {
  return bringChatsStore.use();
}

export function clearBringChatsRequest(request: BringChatsRequest): void {
  bringChatsStore.clear(request);
}
