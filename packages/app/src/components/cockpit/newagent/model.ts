// The New agent page: the rules that need no screen. Greeting, example tasks, which agent can start, the run-as
// choice, pinned home cards, and the request the page hands to whoever starts the agent. No React, no IPC.
import type { Account } from '../../../lib/accounts';
import type { AgentDescriptor } from '../../../lib/agentRegistry';

// ---------- greeting ----------

/** The longest name kept (the settings schema says the same). */
export const MAX_NAME_CHARS = 60;

/** The name as saved: trimmed, one line, not too long. Empty means "no name". */
export function cleanName(raw: string | null | undefined): string {
  return (raw ?? '').replace(/\s+/g, ' ').trim().slice(0, MAX_NAME_CHARS);
}

/**
 * "What's up next, Sam?" with a name, "What's up next?" without. The name only ever comes from the setting the
 * person typed; nothing here looks at the computer or the account.
 */
export function greeting(name: string | null | undefined): string {
  const who = cleanName(name);
  return who ? `What’s up next, ${who}?` : 'What’s up next?';
}

// ---------- example tasks ----------

/** Plain example tasks for the empty box. Tab fills the one on screen. */
export const EXAMPLES: readonly string[] = [
  'Find out why the tests are failing and fix them.',
  'Add a short section to the README on how to run this project.',
  'Explain how this project is organised, in a few lines.',
  'Rename a function and update everything that calls it.',
  'Look through the last few commits and write release notes.',
  'Add a test for the part of the code that has none.',
];

/** How long one example stays before the next one fades in. */
export const EXAMPLE_MS = 6000;

/** The index after `index`, wrapping round. Anything odd starts again at the first. */
export function nextExample(index: number, count: number = EXAMPLES.length): number {
  if (!Number.isInteger(index) || count <= 0) return 0;
  return (index + 1) % count;
}

/** The example on screen for `index` (wrapped, so any number is safe). */
export function exampleAt(index: number): string {
  const n = EXAMPLES.length;
  return EXAMPLES[((Math.trunc(index) % n) + n) % n];
}

// ---------- agents ----------

/** One row of the agent picker: an agent, or an agent under one of its saved logins. */
export interface AgentChoice {
  /** `agentId` or `agentId:accountId`. */
  key: string;
  agentId: string;
  accountId?: string;
  label: string;
  /** `ready`: can start now. `missing`: the program is not installed. `unavailable`: cannot open a pane. */
  state: 'ready' | 'missing' | 'unavailable';
  hint: string;
}

/**
 * The picker's rows from the agent registry. An agent with saved logins gets its default row first, then one row
 * per login. Agents that cannot open a visible pane are listed but cannot be picked.
 */
export function agentChoices(agents: readonly AgentDescriptor[], accounts: readonly Account[]): AgentChoice[] {
  const out: AgentChoice[] = [];
  for (const agent of agents) {
    if (!agent.modes.visible && !agent.modes.headless) continue;
    // A shell is not an agent: a message typed into it would run as a command.
    if (agent.kind === 'shell') continue;
    const label = agent.label || agent.id;
    const state: AgentChoice['state'] = !agent.modes.visible ? 'unavailable' : agent.installed ? 'ready' : 'missing';
    const hint = state === 'ready' ? 'ready' : state === 'missing' ? 'not installed' : 'only for tasks from other agents';
    const logins = accounts.filter((a) => a.agent_id === agent.id);
    out.push({ key: agent.id, agentId: agent.id, label, state, hint });
    for (const login of logins) {
      out.push({ key: `${agent.id}:${login.id}`, agentId: agent.id, accountId: login.id, label: `${label} · ${login.label}`, state, hint });
    }
  }
  return out;
}

/** The choice to start with: the remembered one if it can start, else the first that can. */
export function defaultChoice(choices: readonly AgentChoice[], remembered: string | null): AgentChoice | null {
  const ok = (c: AgentChoice) => c.state === 'ready';
  return choices.find((c) => c.key === remembered && ok(c)) ?? choices.find(ok) ?? null;
}

// ---------- run as ----------

export type RunAs = 'terminal' | 'chat';

/** What `agent_lane_get` says, trimmed to what the page uses. */
export interface LaneView {
  labsEnabled: boolean;
  agents: { id: string; lane: 'terminal' | 'acp'; disabled: boolean }[];
}

/** Whether the Terminal | Chat switch is drawn for this agent: Chat is on in Labs and the agent has a chat way to run. */
export function chatAvailable(lanes: LaneView | null, agentId: string | null): boolean {
  if (!lanes || !lanes.labsEnabled || !agentId) return false;
  const found = lanes.agents.find((a) => a.id.toLowerCase() === agentId.toLowerCase());
  return !!found && !found.disabled;
}

/** The way the agent runs unless the person flips the switch: its saved choice. */
export function defaultRunAs(lanes: LaneView | null, agentId: string | null): RunAs {
  if (!chatAvailable(lanes, agentId)) return 'terminal';
  const found = lanes!.agents.find((a) => a.id.toLowerCase() === agentId!.toLowerCase());
  return found?.lane === 'acp' ? 'chat' : 'terminal';
}

// ---------- the request ----------

/** What the page asks for when the person sends. */
export interface StartRequest {
  agentId: string;
  accountId?: string;
  /** The project folder; absent: the app's default folder. */
  folder?: string;
  /** Work in a separate copy of the project. */
  isolated: boolean;
  runAs: RunAs;
  /** What to tell the agent first (already trimmed, never empty). */
  prompt: string;
}

/** Why Send is off right now, or null when the person can send. */
export function sendBlock(input: { prompt: string; choice: AgentChoice | null; runAs: RunAs; folder: string | null; busy: boolean }): string | null {
  if (input.busy) return 'Starting…';
  if (!input.choice) return 'No agent is ready. Pick one or set one up.';
  if (input.choice.state !== 'ready') return `${input.choice.label} is ${input.choice.hint}.`;
  if (input.runAs === 'chat' && !input.folder) return 'Pick a folder for a chat session.';
  if (!input.prompt.trim()) return 'Write what the agent should do.';
  return null;
}

/** A path as it is put in the message: quoted when it has a space. */
export function pathForMessage(path: string): string {
  return /\s/.test(path) ? `"${path}"` : path;
}

/** `text` with `paths` added, one per line after what is already written. */
export function withPaths(text: string, paths: readonly string[]): string {
  if (paths.length === 0) return text;
  const lines = paths.map(pathForMessage).join('\n');
  if (!text.trim()) return `${lines}\n`;
  return `${text.replace(/\s+$/, '')}\n${lines}\n`;
}

// ---------- home cards ----------

export type CardKind = 'recent' | 'running' | 'needs' | 'prompt';

export interface HomeCard {
  id: string;
  kind: CardKind;
  /** A saved prompt's text. */
  text?: string;
}

export const MAX_CARDS = 12;
export const MAX_PROMPT_CHARS = 2000;

export const CARD_KINDS: readonly { kind: CardKind; label: string; hint: string }[] = [
  { kind: 'recent', label: 'Recent projects', hint: 'Your last folders, one click to use.' },
  { kind: 'running', label: 'Running agents', hint: 'Who is working now.' },
  { kind: 'needs', label: 'Needs you', hint: 'Agents waiting for an answer.' },
  { kind: 'prompt', label: 'A saved prompt', hint: 'Text you use often.' },
];

export const cardTitle = (card: HomeCard): string => CARD_KINDS.find((k) => k.kind === card.kind)?.label ?? 'Card';

/** Cards as they were saved, with anything unreadable dropped. Never throws. */
export function parseCards(raw: unknown): HomeCard[] {
  if (!Array.isArray(raw)) return [];
  const out: HomeCard[] = [];
  const seen = new Set<string>();
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue;
    const { id, kind, text } = item as Record<string, unknown>;
    if (typeof id !== 'string' || !id || seen.has(id)) continue;
    if (kind !== 'recent' && kind !== 'running' && kind !== 'needs' && kind !== 'prompt') continue;
    seen.add(id);
    const card: HomeCard = { id, kind };
    if (kind === 'prompt') card.text = typeof text === 'string' ? text.slice(0, MAX_PROMPT_CHARS) : '';
    out.push(card);
  }
  return out.slice(0, MAX_CARDS);
}

/** A card id nobody else has. */
export function newCardId(cards: readonly HomeCard[], seed: number = Date.now()): string {
  let n = seed;
  const used = new Set(cards.map((c) => c.id));
  while (used.has(`c${n.toString(36)}`)) n += 1;
  return `c${n.toString(36)}`;
}

/** Whether another card of this kind may be added: the list is not full, and only a prompt card may repeat. */
export function canAdd(cards: readonly HomeCard[], kind: CardKind): boolean {
  if (cards.length >= MAX_CARDS) return false;
  return kind === 'prompt' || !cards.some((c) => c.kind === kind);
}

export function addCard(cards: readonly HomeCard[], kind: CardKind, text?: string, id: string = newCardId(cards)): HomeCard[] {
  if (!canAdd(cards, kind)) return [...cards];
  if (kind === 'prompt') {
    const body = (text ?? '').trim().slice(0, MAX_PROMPT_CHARS);
    if (!body) return [...cards];
    return [...cards, { id, kind, text: body }];
  }
  return [...cards, { id, kind }];
}

export const removeCard = (cards: readonly HomeCard[], id: string): HomeCard[] => cards.filter((c) => c.id !== id);

/** Moves a card one place up (-1) or down (+1). At an end it stays. */
export function moveCard(cards: readonly HomeCard[], id: string, by: -1 | 1): HomeCard[] {
  const at = cards.findIndex((c) => c.id === id);
  const to = at + by;
  if (at < 0 || to < 0 || to >= cards.length) return [...cards];
  const next = [...cards];
  [next[at], next[to]] = [next[to], next[at]];
  return next;
}
