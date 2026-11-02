// A chat session over the Agent Client Protocol (the "Chat (ACP)" way to run an agent): the typed commands, the state
// event, and the pure rules the views use. The conversation itself is the ordinary session chat (`sessionChat.ts`,
// source `acp`); a question the agent asks is an ordinary question card (kind `acp_permission`). This file is only
// what is new: stop, the session's state, and which cards belong to a pane.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Question } from './questions';
import { blockOf } from './acpAddon';
import { sendPaneMessage } from './paneOutbox';

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
  /** The agent has said nothing for this many seconds and the person is asked: Stop or Keep waiting. Absent/null: not asked. */
  quietSecs?: number | null;
  /** The agent takes text while it works (a message is then sent into the running turn). */
  canSteer: boolean;
  turn: number;
  title: string | null;
  usage: { used: number; size: number } | null;
  permissions: AcpPermission[];
  closed: boolean;
  /** Why the adapter is not running (phase `exited`): `closed` | `crashed` | `hung` | `write_stalled` | `start_failed`. */
  exit?: { kind: string; code: number | null; reason: string | null } | null;
  /** The agent refused the last message until the person signs in (the sign-in card shows). */
  signIn?: { needed: boolean; hint: string };
  /** The saved conversation's id (== the pane id) when the chat is a saved conversation, else null. */
  conversation?: string | null;
  /** The conversation was handed to a terminal pane: this chat is paused. `paneId` is that terminal, when known. */
  inTerminal?: { paneId: string | null } | null;
  /** An agent (one that started this chat through Puppet Master, or any agent tool) may answer the permission cards. Off by default. */
  agentAnswering?: boolean;
  /** The app closed during a turn (found in the chat journal). The chat says so and offers "Continue" until a new turn starts. */
  interruptedTurn?: boolean;
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

/** Every saved "How to run" value (`agent_lane_get`/`agent_lane_set`): the two above plus Chat (terminal). */
export type RunLaneWord = AcpLane | 'terminal_chat';

/** One agent that has an enabled ACP definition (`agent_lane_get`). */
export interface AcpLaneAgent {
  id: string;
  label: string;
  /** The saved choice. */
  lane: RunLaneWord;
  /** What a new session really uses (the choice, unless something stops it). */
  effective_lane: RunLaneWord;
  disabled: boolean;
  disabled_note: string | null;
  /** The chat comes from the person's own add-on (`acp_overlay_set`), so it can be removed. */
  overlay?: boolean;
  /** The chat add-on files this agent has on disk (the Remove sheet offers to delete them). Absent: none. */
  addon?: { installed: boolean; size_mb: number };
}

/** What `acp_chat_remove` did (it comes with the lane view as `removal`). */
export interface AcpRemoval {
  chats_stopped: number;
  conversations: number;
  overlay_removed: boolean;
  addon_deleted: boolean | null;
  addon_error: string | null;
}

/** An agent that has no chat yet and could get one (`reason`: why it cannot, then the button is off). */
export interface AcpAddable {
  id: string;
  label: string;
  reason?: string;
}

/** An agent that can run as "Chat (terminal)": a normal terminal pane whose Chat tab reads its session file (`agent_lane_get`). */
export interface TerminalChatAgent {
  id: string;
  label: string;
  reads_file: boolean;
  can_resume: boolean;
  /** The profile has a prompt parser, so a question can be answered from the chat. */
  can_answer_prompts: boolean;
  note: string;
  /** The saved "How to run" is Chat (terminal). */
  chosen: boolean;
}

/** `PaneInfo.lane` of a Chat (terminal) pane. */
export const TERMINAL_CHAT_LANE = 'terminal_chat';
/** The name of this way to run, in the switch and the "How to run" popup. Never the agent's product name, never "ACP". */
export const TERMINAL_CHAT_WORD = 'Chat (terminal)';
/** The grey badge on a Chat (terminal) pane's header. */
export const TERMINAL_CHAT_BADGE = 'Chat (terminal)';

export interface AcpLaneView {
  labs_enabled: boolean;
  agents: AcpLaneAgent[];
  /** Agents that can run as Chat (terminal). An older backend sends none. */
  terminal_chat?: TerminalChatAgent[];
  /** Agents with no chat yet. An older backend sends none. */
  can_add_chat?: AcpAddable[];
}

/** One guess of how an agent starts as a chat, from its help text (`acp_overlay_detect`). */
export interface OverlayDetect {
  ok: boolean;
  command: string;
  candidates: { command: string; args: string[]; evidence: string; confidence: 'high' | 'medium' }[];
  stdio_hint?: string | null;
  mentions_protocol?: boolean;
}

/** What the official ACP registry says about a ready-made adapter (`acp_overlay_resolve`). Plain text only: render, never run. */
export interface RegistryInfo {
  id: string;
  name: string;
  version: string;
  authors: string[];
  license: string;
  repository?: string;
  distribution: 'npx' | 'binary' | 'uvx';
  /** Roughly how big the download is, when known. */
  size_hint?: string | null;
}

/** What the built-in bridge gives an agent: a chat over its non-interactive mode, text only. */
export interface BridgeInfo {
  /** A later message continues the earlier ones. False: each message starts fresh. */
  has_memory: boolean;
  read_only_supported?: boolean;
  note?: string;
}

/** One way to give an agent a chat (`acp_overlay_resolve`), best first. */
export interface OverlayOption {
  /** `native`: the program you have already starts chat; `registry`: a download from the official ACP registry; `bridge`: the built-in bridge. */
  kind: 'native' | 'registry' | 'terminal_chat' | 'bridge';
  label: string;
  summary: string;
  command_line?: string;
  /** `native`: the exact program and arguments to save. */
  acp?: OverlayAcp;
  registry?: RegistryInfo;
  /** `bridge`: what the made-by-Puppet-Master chat will be like. */
  bridge?: BridgeInfo;
  /** `terminal_chat`: what Chat (terminal) gives this agent. */
  terminal_chat?: { reads_file: boolean; can_resume: boolean; can_answer_prompts: boolean; note: string };
  available: boolean;
  /** Why it is not available. */
  reason?: string;
}

export interface OverlayResolve {
  ok: boolean;
  options: OverlayOption[];
}

/** What the person confirmed for a download. `id` and `version` are what the screen showed; the app refuses if the registry moved. */
export type OverlayInstallOption = { kind: 'registry'; id?: string; version?: string } | { kind: 'bridge' } | { kind: 'terminal_chat' };

/** The confirm text for the built-in bridge. */
export function bridgeSentence(label: string, hasMemory: boolean): string {
  return `${label} has no chat support of its own, so Puppet Master will make one. It talks to ${label} one message at a time: text only, no tool cards, ${hasMemory ? 'it remembers the conversation' : 'it does not remember earlier messages'}.`;
}

/** Progress of a download, from the `acp-addon-changed` event. */
export interface OverlayProgress {
  state: 'installing' | 'installed' | 'failed' | 'not_installed';
  lines: string[];
  detail: string;
}

/** The first option that can be used now, or -1. */
export function firstAvailable(options: readonly OverlayOption[] | null | undefined): number {
  return (options ?? []).findIndex((o) => o && o.available === true);
}

/** Clean list from the wire: unknown kinds dropped, text fields defaulted. */
export function normalizeOptions(raw: OverlayResolve | null | undefined): OverlayOption[] {
  const list = Array.isArray(raw?.options) ? raw!.options : [];
  return list
    .filter((o) => o && (o.kind === 'native' || o.kind === 'registry' || o.kind === 'terminal_chat' || o.kind === 'bridge'))
    .map((o) => ({ ...o, label: String(o.label ?? ''), summary: String(o.summary ?? ''), available: o.available === true }));
}

/** "Alice, Bob" (at most three names, then "and others"); empty when the registry names nobody. */
export function authorsText(authors: readonly string[] | null | undefined): string {
  const names = (authors ?? []).filter((a) => typeof a === 'string' && a.trim() !== '');
  if (names.length === 0) return '';
  return names.length > 3 ? `${names.slice(0, 3).join(', ')} and others` : names.join(', ');
}

/** The sentence on the confirm screen for a download from the registry. */
export function registrySentence(info: RegistryInfo): string {
  const by = authorsText(info.authors);
  const license = info.license?.trim() ? ` (${info.license.trim()})` : '';
  const size = info.size_hint?.trim() ? ` About ${info.size_hint.trim()}.` : '';
  return `Puppet Master will download ${info.name} ${info.version}${by ? ` by ${by}` : ''}${license} from the official ACP registry and run it for chat.${size}`;
}

/** The `[acp]` table the person confirmed. */
export interface OverlayAcp {
  command: string;
  args: string[];
  env_allow: string[];
  resume_command?: string[];
}

/** A lane view from the wire with every newer field defaulted (an old backend must not break the screens). */
export function normalizeLaneView(raw: AcpLaneView | null | undefined): AcpLaneView {
  const agents = Array.isArray(raw?.agents) ? raw!.agents : [];
  const addable = Array.isArray(raw?.can_add_chat) ? raw!.can_add_chat.filter((a) => a && typeof a.id === 'string') : [];
  const terminalChat = Array.isArray(raw?.terminal_chat) ? raw!.terminal_chat.filter((a) => a && typeof a.id === 'string') : [];
  return { labs_enabled: raw?.labs_enabled === true, agents, can_add_chat: addable, terminal_chat: terminalChat };
}

/** The agents that could get a chat (Labs on), in the backend's order. */
export function addableChat(view: AcpLaneView | null): AcpAddable[] {
  return view && view.labs_enabled ? (view.can_add_chat ?? []) : [];
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
  /** "Keep waiting" on a quiet agent: the quiet window starts again. */
  keepWaiting?: (paneId: string) => Promise<boolean>;
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
  laneSet: (agentId: string, lane: RunLaneWord) => Promise<AcpLaneView>;
  /** Start a chat session. `explicit`: the person picked Chat for this session, so the saved choice is not needed. */
  start: (agentId: string, folder: string, explicit?: boolean) => Promise<AcpStarted>;
  /** Read the agent's help text for how it starts as a chat. Runs nothing but help. */
  overlayDetect?: (agent: string) => Promise<OverlayDetect>;
  /** Every way to give the agent a chat (its own program, the official ACP registry, the built-in bridge), best first. */
  overlayResolve?: (agent: string) => Promise<OverlayResolve>;
  /** Download and set up the registry's adapter. Refused unless `confirmed`; resolves when it is installed, rejects with the reason. */
  overlayInstall?: (agent: string, option: OverlayInstallOption, confirmed: boolean) => Promise<unknown>;
  /** Progress lines of a download for `agent`. Returns the stop function. */
  overlayProgress?: (agent: string, cb: (p: OverlayProgress) => void) => Promise<UnlistenFn>;
  /** Give an agent a chat. Refused (rejects with the reason) unless `confirmed`. Answers with the agent's lane row. */
  overlaySet?: (agent: string, acp: OverlayAcp, confirmed: boolean) => Promise<AcpLaneAgent>;
  /** Take the added chat away again. */
  overlayRemove?: (agent: string) => Promise<void>;
  /** Remove Chat for any agent: stops its chats, back to Terminal, drops an added chat, and with `removeAddon` deletes the add-on files. */
  chatRemove?: (agent: string, opts: { removeAddon: boolean }) => Promise<AcpLaneView & { removal?: AcpRemoval }>;
  /** "Restart chat": brings the adapter of a stopped chat back. */
  restart?: (paneId: string) => Promise<AcpChatState>;
  /** "Continue" after the app closed in the middle of a turn: the person's click sends one ordinary message. */
  continueTurn?: (paneId: string) => Promise<unknown>;
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
  keepWaiting: (paneId) => call<boolean>('acp_chat_keep_waiting', { paneId }),
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
  laneGet: async () => normalizeLaneView(await call<AcpLaneView>('agent_lane_get')),
  laneSet: async (agentId, lane) => normalizeLaneView(await call<AcpLaneView>('agent_lane_set', { id: agentId, lane })),
  start: (agentId, folder, explicit) => call<AcpStarted>('acp_chat_start', { agentId, folder, explicit: explicit ?? false }),
  overlayDetect: (agent) => call<OverlayDetect>('acp_overlay_detect', { agent }),
  overlayResolve: async (agent) => ({ ok: true, options: normalizeOptions(await call<OverlayResolve>('acp_overlay_resolve', { agent })) }),
  overlayInstall: (agent, option, confirmed) => call<unknown>('acp_overlay_install', { agent, option, confirmed }),
  overlayProgress: async (agent, cb) => {
    if (!isTauri()) return noopUnlisten;
    return listen<{ agent: string; status?: { state?: string; lines?: unknown; detail?: unknown } }>('acp-addon-changed', (e) => {
      if (String(e.payload?.agent ?? '').toLowerCase() !== agent.toLowerCase()) return;
      const st = e.payload?.status;
      const state = st?.state === 'installing' || st?.state === 'installed' || st?.state === 'failed' ? st.state : 'not_installed';
      cb({ state, lines: Array.isArray(st?.lines) ? st!.lines.filter((l): l is string => typeof l === 'string') : [], detail: typeof st?.detail === 'string' ? st.detail : '' });
    });
  },
  overlaySet: (agent, acp, confirmed) => call<AcpLaneAgent>('acp_overlay_set', { agent, acp, confirmed }),
  overlayRemove: async (agent) => {
    await call<unknown>('acp_overlay_remove', { agent });
  },
  chatRemove: async (agent, opts) => {
    const view = await call<AcpLaneView & { removal?: AcpRemoval }>('acp_chat_remove', { agent, opts: { remove_addon: opts.removeAddon } });
    return { ...normalizeLaneView(view), removal: view.removal };
  },
  restart: (paneId) => call<AcpChatState>('acp_chat_restart', { paneId }),
  continueTurn: (paneId) => sendPaneMessage(paneId, CONTINUE_TEXT),
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

/**
 * The question under a turn whose agent has said nothing for the quiet window: "No answer for 5 min", with Stop and
 * Keep waiting. `null` when the agent is not quiet, or a stop is already on its way.
 */
export function quietStrip(state: AcpChatState | null, asked = false): { text: string } | null {
  if (!state || state.closed || !state.turnInFlight || state.stopping || asked) return null;
  if (state.phase === 'needs_input' || !state.quietSecs) return null;
  return { text: `No answer for ${Math.max(1, Math.round(state.quietSecs / 60))} min` };
}

const STOPPED_WHY: Record<string, string> = {
  crashed: 'the agent stopped unexpectedly',
  hung: 'the agent stopped answering',
  write_stalled: 'the agent stopped reading messages',
  start_failed: 'the agent could not start',
};

/**
 * The line shown when the adapter is gone for a reason that is not the person's doing: "The chat stopped: <why>" with a
 * Restart button. `null` while it runs, was closed on purpose, or is only napping. No spinner goes with it.
 */
export function stoppedStrip(state: AcpChatState | null): { text: string } | null {
  if (!state || state.closed || state.phase !== 'exited' || !state.exit) return null;
  const why = STOPPED_WHY[state.exit.kind];
  if (!why) return null;
  const detail = state.exit.kind === 'start_failed' && state.exit.reason ? `: ${state.exit.reason.replace(/\s+/g, ' ').trim().slice(0, 160)}` : state.exit.kind === 'crashed' && state.exit.code != null ? ` (exit code ${state.exit.code})` : '';
  return { text: `The chat stopped: ${why}${detail}` };
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

/** Is this pane a Chat (terminal) pane? Takes the raw `PaneInfo.lane` (its type only knows the ACP lane). */
export function isTerminalChatLane(lane: string | null | undefined): boolean {
  return lane === TERMINAL_CHAT_LANE;
}

/** The agents that can run as Chat (terminal): Labs on. An older backend sends none. */
export function terminalChatCapable(view: AcpLaneView | null): TerminalChatAgent[] {
  return view && view.labs_enabled && Array.isArray(view.terminal_chat) ? view.terminal_chat : [];
}

/** Start an agent as Chat (terminal) (`terminal_chat_start`): a normal terminal pane. Resolves to the new pane id. */
export function startTerminalChat(agentId: string, cwd?: string): Promise<string> {
  return call<string>('terminal_chat_start', { agent: agentId, cwd: cwd ?? null });
}

// ---- The app closed during a turn (P-29) --------------------------------------------------------------------------

/** What "Continue" sends: an ordinary message, sent by the person's click. */
export const CONTINUE_TEXT = 'Continue where you left off.';

/** The line under the chat after a restart that found a turn the app never saw end: `null` when there is none. */
export function interruptedStrip(state: AcpChatState | null): { text: string } | null {
  if (!state || state.closed || !state.interruptedTurn || state.turnInFlight) return null;
  return { text: 'The app closed during this turn.' };
}
