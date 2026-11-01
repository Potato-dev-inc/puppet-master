// A chat session over the Agent Client Protocol (the "Chat (ACP)" way to run an agent): the typed commands, the state
// event, and the pure rules the views use. The conversation itself is the ordinary session chat (`sessionChat.ts`,
// source `acp`); a question the agent asks is an ordinary question card (kind `acp_permission`). This file is only
// what is new: stop, the session's state, and which cards belong to a pane.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Question } from './questions';
import { blockOf } from './acpAddon';

/** `PaneInfo.lane` of a chat session. */
export const ACP_LANE = 'acp';
/** `Question.kind` of the card for the agent's own permission request. */
export const ACP_PERMISSION_KIND = 'acp_permission';
/** The badge on the pane (the name of the way to run, never an agent's product name). */
export const ACP_BADGE = 'Chat (ACP)';

/** One permission question of the agent, as the session reports it. */
export interface AcpPermission {
  askId: number;
  title: string | null;
  toolKind: string | null;
  /** The question card that stands for it (absent for a moment before it is raised). */
  questionId: string | null;
  options: { id: string; name: string; kind: string }[] | null;
  sinceMs: number;
}

/** The session behind a chat pane (Rust `AcpChat::state`). */
export interface AcpChatState {
  lane: 'acp';
  agent: string;
  label: string;
  /** starting, idle, working, needs_input, cancelling, resuming, dormant, exited. */
  phase: string;
  status: 'running' | 'waiting_input' | 'idle' | 'error';
  turnInFlight: boolean;
  /** Stop was asked and the agent has not ended the turn yet. */
  stopping: boolean;
  /** The agent takes text while it works (a message is then sent into the running turn). */
  canSteer: boolean;
  turn: number;
  title: string | null;
  usage: { used: number; size: number } | null;
  permissions: AcpPermission[];
  closed: boolean;
  /** The agent refused the last message until the person signs in (the sign-in card shows). */
  signIn?: { needed: boolean; hint: string };
  /** The saved conversation's id (== the pane id) when the chat is a saved conversation, else null. */
  conversation?: string | null;
  /** The conversation was handed to a terminal pane: this chat is paused. `paneId` is that terminal, when known. */
  inTerminal?: { paneId: string | null } | null;
  /** An agent (one that started this chat through Puppet Master, or any agent tool) may answer the permission cards. Off by default. */
  agentAnswering?: boolean;
  /** The agent's own settings (model, mode, effort), as ACP reports them. */
  configOptions?: AcpConfigOption[];
  /** The slash commands the agent offers. */
  commands?: AcpCommand[];
}

/** One choice of a config option. */
export interface AcpConfigChoice {
  value: string;
  name: string;
  description?: string | null;
}

/** An agent setting in ACP's raw shape. A grouped list or a type other than `select` is ignored by `configChips`. */
export interface AcpConfigOption {
  id: string;
  name: string;
  description?: string | null;
  /** `model`, `mode`, `thought_level` or something else. */
  category?: string | null;
  type: string;
  currentValue: string;
  options: unknown[];
}

/** A slash command the agent offers. */
export interface AcpCommand {
  name: string;
  description: string;
  /** What to type after the name (shown as the box's hint once the command is picked), or null. */
  hint: string | null;
}

/** The lane an agent runs in. */
export type AcpLane = 'terminal' | 'acp';

/** One agent that has an enabled ACP definition (`agent_lane_get`). */
export interface AcpLaneAgent {
  id: string;
  label: string;
  /** The saved choice. */
  lane: AcpLane;
  /** What a new session really uses (the choice, unless something stops it). */
  effective_lane: AcpLane;
  disabled: boolean;
  disabled_note: string | null;
}

export interface AcpLaneView {
  labs_enabled: boolean;
  agents: AcpLaneAgent[];
}

/** `acp_chat_start`. */
export interface AcpStarted {
  pane_id: string;
  agent: string;
  lane: 'acp';
}

/** An answer to a hand-over: done, or "ask the person first". */
export type HandOver<T> = { ok: true; result: T } | { ok: false; ask: string; reason: string };
export interface OpenedInTerminal {
  conversation: string;
  pane_id: string;
  agent: string;
  session_id: string;
}
export interface BackInChat {
  conversation: string;
  session_id: string;
  how: string;
}
/** `acp_conversation_of_terminal`: the chat conversation a terminal pane holds. */
export interface TerminalConversation {
  conversation: string;
  agent: string;
}

export interface AcpChatApi {
  state: (paneId: string) => Promise<AcpChatState | null>;
  cancel: (paneId: string) => Promise<boolean>;
  /** "I'm signed in — try again": sends the message that was waiting. Rejects with the agent's reason. */
  retrySignIn?: (paneId: string) => Promise<void>;
  watch: (cb: (paneId: string, state: AcpChatState) => void) => Promise<UnlistenFn>;
  /** Change one of the agent's settings (model, mode, effort). Refused with `BUSY…` while a turn runs. */
  setConfig: (paneId: string, optionId: string, value: string) => Promise<AcpChatState>;
  /** Let (or stop letting) an agent answer this chat's permission questions. */
  setAgentAnswering: (paneId: string, allowed: boolean) => Promise<AcpChatState>;
  /** Hand the conversation to a terminal pane. `force` stops the running turn first. */
  openInTerminal: (conversation: string, force?: boolean) => Promise<HandOver<OpenedInTerminal>>;
  /** Hand the conversation back to its chat pane. `force` stops the terminal first. */
  backToChat: (conversation: string, force?: boolean) => Promise<HandOver<BackInChat>>;
  /** Which chat conversation an ordinary terminal pane holds, if any. */
  conversationOfTerminal: (paneId: string) => Promise<TerminalConversation | null>;
  /** Fires after every hand-over (no payload). */
  watchConversations: (cb: () => void) => Promise<UnlistenFn>;
  laneGet: () => Promise<AcpLaneView>;
  laneSet: (agentId: string, lane: AcpLane) => Promise<AcpLaneView>;
  /** Start a chat session. `explicit`: the person picked Chat for this session, so the saved choice is not needed. */
  start: (agentId: string, folder: string, explicit?: boolean) => Promise<AcpStarted>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const tauriAcpChatApi: AcpChatApi = {
  state: async (paneId) => {
    if (!isTauri()) return null;
    return (await invoke<AcpChatState | null>('acp_chat_state', { paneId })) ?? null;
  },
  cancel: (paneId) => call<boolean>('acp_chat_cancel', { paneId }),
  retrySignIn: async (paneId) => {
    if (!isTauri()) throw new Error('Desktop shell required: acp_chat_retry_sign_in');
    await invoke<void>('acp_chat_retry_sign_in', { paneId });
  },
  watch: async (cb) => {
    if (!isTauri()) return noopUnlisten;
    return listen<{ paneId: string; state: AcpChatState }>('acp-chat-state', (e) => cb(e.payload.paneId, e.payload.state));
  },
  setConfig: (paneId, optionId, value) => call<AcpChatState>('acp_chat_set_config', { paneId, optionId, value }),
  setAgentAnswering: (paneId, allowed) => call<AcpChatState>('acp_chat_set_agent_answering', { paneId, allowed }),
  openInTerminal: (conversation, force) => call<HandOver<OpenedInTerminal>>('acp_open_in_terminal', { conversation, force: force ?? false }),
  backToChat: (conversation, force) => call<HandOver<BackInChat>>('acp_back_to_chat', { conversation, force: force ?? false }),
  conversationOfTerminal: async (paneId) => {
    if (!isTauri()) return null;
    return (await invoke<TerminalConversation | null>('acp_conversation_of_terminal', { paneId })) ?? null;
  },
  watchConversations: async (cb) => {
    if (!isTauri()) return noopUnlisten;
    return listen('acp-conversations-changed', () => cb());
  },
  laneGet: () => call<AcpLaneView>('agent_lane_get'),
  laneSet: (agentId, lane) => call<AcpLaneView>('agent_lane_set', { id: agentId, lane }),
  start: (agentId, folder, explicit) => call<AcpStarted>('acp_chat_start', { agentId, folder, explicit: explicit ?? false }),
};

/** The permission cards that belong to `paneId`, oldest first. */
export function permissionCards(questions: readonly Question[], paneId: string): Question[] {
  return questions.filter((q) => q.kind === ACP_PERMISSION_KIND && q.pane_id === paneId && q.state === 'pending');
}

/**
 * What the strip above the message box says and does. `null`: nothing runs, no strip. `asked`: the person pressed
 * Stop and the backend's state has not come back yet, so the strip already reads "Stopping…".
 */
export function runStrip(state: AcpChatState | null, asked = false): { text: string; stopping: boolean } | null {
  if (!state || state.closed || !state.turnInFlight) return null;
  if (state.stopping || asked) return { text: 'Stopping…', stopping: true };
  if (state.phase === 'needs_input') return { text: 'Waiting for your answer', stopping: false };
  return { text: 'Working…', stopping: false };
}

// ---- Settings chips (model, mode, effort) -------------------------------------------------------------------------

/** One compact select under the message box. */
export interface ConfigChip {
  id: string;
  /** The setting's name ("Model"), used as the label for screen readers. */
  label: string;
  category: string | null;
  value: string;
  choices: { value: string; name: string; description: string | null }[];
}

function plainChoices(options: unknown[]): ConfigChip['choices'] | null {
  const out: ConfigChip['choices'] = [];
  for (const o of options) {
    if (!o || typeof o !== 'object') return null;
    const { value, name, description } = o as Record<string, unknown>;
    // A grouped list (`{group, options}`) has no `value`: the whole setting is left out.
    if (typeof value !== 'string' || typeof name !== 'string') return null;
    out.push({ value, name, description: typeof description === 'string' ? description : null });
  }
  return out;
}

const CATEGORY_ORDER: Record<string, number> = { model: 0, mode: 1, thought_level: 2 };

/** The settings to show as chips: plain `select`s with at least two choices. Model first, then mode, then effort. */
export function configChips(state: AcpChatState | null): ConfigChip[] {
  const chips: ConfigChip[] = [];
  for (const option of state?.configOptions ?? []) {
    if (!option || option.type !== 'select' || !Array.isArray(option.options)) continue;
    const choices = plainChoices(option.options);
    if (!choices || choices.length < 2) continue;
    chips.push({ id: option.id, label: option.name || option.id, category: option.category ?? null, value: option.currentValue, choices });
  }
  const rank = (chip: ConfigChip) => CATEGORY_ORDER[chip.category ?? ''] ?? 9;
  // `sort` is stable: settings of one kind keep the order the agent listed them in.
  return chips.sort((a, b) => rank(a) - rank(b));
}

/** The name of the choice a chip currently holds (its value when the agent lists none by that value). */
export function chipValueName(chip: ConfigChip): string {
  return chip.choices.find((c) => c.value === chip.value)?.name ?? chip.value;
}

/** The chips are switched off while the agent works: ACP changes settings between turns. */
export function chipsDisabled(state: AcpChatState | null): boolean {
  return !state || state.turnInFlight || state.closed || !!state.inTerminal;
}

// ---- Slash commands -----------------------------------------------------------------------------------------------

const bare = (name: string): string => name.replace(/^\/+/, '');

/**
 * The text typed after the first "/" while the person is still choosing a command, or null when the menu does not
 * apply: the text must start with "/" and have no space or line break yet.
 */
export function slashQuery(text: string): string | null {
  if (!text.startsWith('/')) return null;
  const rest = text.slice(1);
  return /\s/.test(rest) ? null : rest;
}

/** The commands whose name starts with what was typed (case-insensitive), in the agent's order. */
export function filterCommands(commands: readonly AcpCommand[], text: string): AcpCommand[] {
  const query = slashQuery(text);
  if (query === null) return [];
  const q = query.toLowerCase();
  return commands.filter((c) => bare(c.name).toLowerCase().startsWith(q));
}

/** What picking a command puts in the box. */
export function commandText(command: AcpCommand): string {
  return `/${bare(command.name)} `;
}

/** The hint to show in the empty rest of the box once `text` is exactly a command and a space. */
export function commandHint(commands: readonly AcpCommand[], text: string): string | null {
  const m = /^\/(\S+) $/.exec(text);
  if (!m) return null;
  return commands.find((c) => bare(c.name).toLowerCase() === m[1].toLowerCase())?.hint ?? null;
}

// ---- Errors, start fallback, hand-over ------------------------------------------------------------------------------

/** A command's error as one plain sentence: the leading `CODE: ` is dropped. */
export function errorSentence(error: unknown): string {
  const text = error instanceof Error ? error.message : typeof error === 'string' ? error : String(error);
  return text.replace(/^[A-Z][A-Z0-9_]*:\s*/, '').trim();
}

const TERMINAL_FALLBACK = 'The terminal can run this agent instead.';

/** A failed start: its sentence, and whether the terminal can run the agent instead (then offer "Start in terminal"). */
export function startFailure(error: unknown): { sentence: string; terminalFallback: boolean } {
  const raw = error instanceof Error ? error.message : typeof error === 'string' ? error : String(error);
  // The chat add-on is not installed: say where to install it, and the terminal still runs the agent.
  if (blockOf(raw) === 'addon') return { sentence: `${errorSentence(error)} You can install it in Settings, under Agents › Chat.`, terminalFallback: true };
  return { sentence: errorSentence(error), terminalFallback: /^ACP_/.test(raw.trim()) && raw.includes(TERMINAL_FALLBACK) };
}

/** Which way a hand-over goes. */
export type HandOverKind = 'toTerminal' | 'toChat';

/** The buttons of the sheet that asks first, in plain words. */
export const HANDOVER_WORDS: Record<HandOverKind, { yes: string; title: string }> = {
  toTerminal: { yes: 'Stop it and open in terminal', title: 'Open in terminal?' },
  toChat: { yes: 'Stop the terminal and go back to chat', title: 'Go back to chat?' },
};

/** The sentence to ask the person when the backend answered `ok: false`, else null. */
export function handOverAsk<T>(reply: HandOver<T>): { ask: string; reason: string } | null {
  return reply.ok ? null : { ask: reply.ask.trim() || 'The agent is busy.', reason: reply.reason };
}

/** The lane a new session starts in for this agent (the agent's setting). */
export function laneChoiceFor(agent: Pick<AcpLaneAgent, 'effective_lane' | 'lane'> | undefined): AcpLane {
  return (agent?.effective_lane ?? agent?.lane) === 'acp' ? 'acp' : 'terminal';
}

/** The agents the person can choose Chat for: Labs on, and not disabled. */
export function chatCapable(view: AcpLaneView | null): AcpLaneAgent[] {
  return view && view.labs_enabled ? view.agents.filter((a) => !a.disabled) : [];
}
