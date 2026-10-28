// The filters of "Search all chats": the chips under the box (Agent, Folder, When, Who) and the typed shortcuts
// (`agent:codex`, `in:demo-a`, `from:me`). Pure helpers; the palette keeps the chips, the box keeps the shortcuts.
// Agent and folder names come from the index (`chatSearchFacets`), never from a list written here.
import type { ChatSearchFilters } from './chatSearch';

export type ChatWhen = 'today' | '7d' | '30d';
export type ChatWho = 'you' | 'agent' | 'tool';

/** Every filter the search can hold. An unset key means "any". */
export interface ChatFilters {
  /** The agent id, as the index keeps it. */
  agent?: string;
  /** The folder name, as the index keeps it. */
  folder?: string;
  when?: ChatWhen;
  who?: ChatWho;
}

/** The names that can be typed or picked, from the index. */
export interface ChatFacetNames {
  agents: string[];
  folders: string[];
}

export const WHEN_LABELS: Record<ChatWhen, string> = { today: 'Today', '7d': '7 days', '30d': '30 days' };
export const WHO_LABELS: Record<ChatWho, string> = { you: 'You', agent: 'Agent', tool: 'Tools' };

const DAY_MS = 86_400_000;

/** The start of the time a "When" chip means, in ms; undefined for "any" (no start time). */
export function whenSince(when: ChatWhen | undefined, now: number): number | undefined {
  if (!when) return undefined;
  if (when === 'today') {
    const day = new Date(now);
    return new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();
  }
  return now - (when === '7d' ? 7 : 30) * DAY_MS;
}

/** The filters the backend takes: only the ones that are set, with the "When" chip turned into a start time. */
export function searchFilters(filters: ChatFilters, now: number): ChatSearchFilters {
  const out: ChatSearchFilters = {};
  if (filters.agent) out.agent = filters.agent;
  if (filters.folder) out.folder = filters.folder;
  const since = whenSince(filters.when, now);
  if (since !== undefined) out.sinceMs = since;
  if (filters.who) out.role = filters.who;
  return out;
}

/** Whether any filter is set. */
export function hasChatFilters(filters: ChatFilters): boolean {
  return Object.values(filters).some((value) => value !== undefined && value !== '');
}

/** The chips with the typed shortcuts on top: a typed shortcut wins over a chip for the same filter. */
export function mergeChatFilters(chips: ChatFilters, typed: ChatFilters): ChatFilters {
  const out: ChatFilters = { ...chips };
  for (const key of Object.keys(typed) as (keyof ChatFilters)[]) {
    if (typed[key] !== undefined) (out as Record<string, unknown>)[key] = typed[key];
  }
  return out;
}

/** The typed shortcut words and the filter each one sets. `me` and `you` are the person. */
const SHORTCUT = /(^|\s)(agent|in|from):(?:"([^"]*)"|(\S+))/gi;
const WHO_WORDS: Record<string, ChatWho> = { me: 'you', you: 'you', agent: 'agent', tool: 'tool', tools: 'tool' };

/** The name the index keeps that matches the typed one (any case); the typed text when none does. */
function nameAs(known: string[], typed: string): string {
  return known.find((name) => name.toLowerCase() === typed.toLowerCase()) ?? typed;
}

export interface ParsedChatQuery {
  /** What is left to search for, with the shortcuts taken out. */
  text: string;
  /** The filters the shortcuts set. */
  typed: ChatFilters;
}

/** Takes `agent:`, `in:` and `from:` shortcuts out of the box. A shortcut with a value it does not know stays as text
 * (so `from:robot` is searched for as words), except agent and folder names, which are kept as typed. */
export function parseChatQuery(input: string, known: ChatFacetNames): ParsedChatQuery {
  const typed: ChatFilters = {};
  const rest = input.replace(SHORTCUT, (whole: string, lead: string, key: string, quoted: string | undefined, bare: string | undefined) => {
    const value = (quoted ?? bare ?? '').trim();
    if (!value) return whole;
    switch (key.toLowerCase()) {
      case 'agent':
        typed.agent = nameAs(known.agents, value);
        return lead;
      case 'in':
        typed.folder = nameAs(known.folders, value);
        return lead;
      default: {
        const who = WHO_WORDS[value.toLowerCase()];
        if (!who) return whole;
        typed.who = who;
        return lead;
      }
    }
  });
  return { text: rest.replace(/\s+/g, ' ').trim(), typed };
}

/** The box with the shortcut for one filter taken out (a chip was picked for it, so the chip rules). */
export function dropShortcut(input: string, key: keyof ChatFilters): string {
  const word = key === 'agent' ? 'agent' : key === 'folder' ? 'in' : key === 'who' ? 'from' : null;
  if (!word) return input;
  const one = new RegExp(`(^|\\s)${word}:(?:"[^"]*"|\\S+)`, 'gi');
  return input.replace(one, '').replace(/\s+/g, ' ').trim();
}
