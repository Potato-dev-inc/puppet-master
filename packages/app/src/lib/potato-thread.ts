// The thread in the middle of the Potato Bot page (header and message list): its words and its pure rules. The thread
// phase keeps its client code here (time and day separators, the unread divider, tool activity, the quote event), not in
// lib/potato.ts. No React in this file.
import type { PotatoMessage } from './potato';

export const THREAD_COPY = {
  messages: 'Messages',
  panelShortcut: 'Ctrl+Shift+B',
  today: 'Today',
  yesterday: 'Yesterday',
  newMessages: 'New',
  jump: 'Jump to latest',
  copy: 'Copy',
  copied: 'Copied',
  reply: 'Reply',
  actionsLabel: 'Message actions',
  details: 'Details',
  hideDetails: 'Hide details',
  needsYou: 'Needs you',
  failed: 'Failed',
  gotResult: 'Got a result',
  quoting: 'Replying to',
  empty: (name: string) => `Say hello to ${name}.`,
};

// ---- time and day separators ----------------------------------------------------------------------

/** A gap of this long between two messages gets a time line ("12:11 AM") between them. */
export const TIME_GAP_MS = 15 * 60 * 1000;

/** "12:11 AM": built by hand so the words are the same on every machine. */
export function clockLabel(at: number): string {
  const d = new Date(at);
  const h = d.getHours();
  const mm = String(d.getMinutes()).padStart(2, '0');
  return `${h % 12 === 0 ? 12 : h % 12}:${mm} ${h < 12 ? 'AM' : 'PM'}`;
}

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

const startOfDay = (at: number): number => {
  const d = new Date(at);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
};

/** Whether two moments fall on the same calendar day (local time). */
export function sameDay(a: number, b: number): boolean {
  return startOfDay(a) === startOfDay(b);
}

/** "Today", "Yesterday", a weekday for the last week, else "Oct 29" (with the year when it is not this year). */
export function dayLabel(at: number, now: number): string {
  const days = Math.round((startOfDay(now) - startOfDay(at)) / 86_400_000);
  if (days === 0) return THREAD_COPY.today;
  if (days === 1) return THREAD_COPY.yesterday;
  const d = new Date(at);
  if (days > 1 && days < 7) return WEEKDAYS[d.getDay()];
  const base = `${MONTHS[d.getMonth()]} ${d.getDate()}`;
  return d.getFullYear() === new Date(now).getFullYear() ? base : `${base}, ${d.getFullYear()}`;
}

// ---- the unread divider ---------------------------------------------------------------------------

/**
 * The id of the first unread message: the bot's messages are counted from the end. Null when nothing is unread, or when
 * the very first message of the thread is unread (a divider above everything says nothing).
 */
export function firstUnreadId(messages: readonly PotatoMessage[], unread: number): string | null {
  if (unread <= 0) return null;
  let left = unread;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].from.kind !== 'bot') continue;
    left -= 1;
    if (left === 0) return i === 0 ? null : messages[i].id;
  }
  return null;
}

// ---- tool activity --------------------------------------------------------------------------------

/** One thing the bot did with a tool, as a person reads it. */
export interface ToolStep {
  id: string;
  tool: string;
  /** "Looked at the screen". */
  label: string;
  /** A short extra, in words ("Fake desktop"). Empty when there is nothing to add. */
  summary: string;
  ok: boolean;
  needsYou: boolean;
  /** The raw result, for the expandable "Details" and nowhere else. Null when there is none. */
  details: string | null;
  group: ToolGroup;
}

export type ToolGroup = 'computer' | 'helper' | 'other';

const TOOL_LABELS: Record<string, string> = {
  computer_observe: 'Looked at the screen',
  computer_act: 'Used the mouse and keyboard',
  browser_navigate: 'Opened a page',
  browser_snapshot: 'Read the page',
  browser_act: 'Used the page',
  ask_person_for_screen: 'Asked you for the screen',
  ask_helper: 'Asked a helper',
  memory_search: 'Looked through its memory',
  memory_open: 'Opened a memory',
  memory_timeline: 'Looked through its memory',
  memory_diary: 'Looked through its diary',
};

export function toolGroup(tool: string): ToolGroup {
  if (tool.startsWith('computer_') || tool.startsWith('browser_') || tool === 'ask_person_for_screen') return 'computer';
  if (tool === 'ask_helper') return 'helper';
  return 'other';
}

export function toolLabel(tool: string): string {
  return TOOL_LABELS[tool] ?? `Used ${tool.replace(/_/g, ' ')}`;
}

const FAILURES: Record<string, string> = {
  LEASE_LOST: 'The screen changed hands.',
  LEASE_NOT_HOLDER: 'It did not have the screen.',
  PERSON_HAS_SCREEN: 'You had the screen.',
  DAILY_LIMIT: 'Hit the daily limit.',
};

function failureWords(code: string): string {
  const known = FAILURES[code];
  if (known) return known;
  const words = code.toLowerCase().replace(/_/g, ' ').trim();
  return words ? `${words.charAt(0).toUpperCase()}${words.slice(1)}.` : 'It did not work.';
}

const SUMMARY_MAX = 110;
const shorten = (text: string, max: number): string => (text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text);

/** Text that is only a JSON object or list. Never shown as words. */
export function isRawJson(text: string): boolean {
  const t = text.trim();
  if (!((t.startsWith('{') && t.endsWith('}')) || (t.startsWith('[') && t.endsWith(']')))) return false;
  try {
    JSON.parse(t);
    return true;
  } catch {
    return false;
  }
}

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text.trim()), null, 2);
  } catch {
    return text.trim();
  }
}

/** Starts like JSON, even if cut short (a result shortened by the brain). */
const looksLikeJson = (text: string): boolean => /^\s*[{[]/.test(text);

// "Fish used computer_observe: ok, saw Fake desktop" / "Fish used x: ok" / "Fish used x: failed LEASE_LOST (needs you)".
const TOOL_LINE = /^[^\n]{1,40}? used ([a-z][a-z0-9_]*): (ok|failed)\b(?:,? ?([\s\S]*))?$/;

/** A tool result written as a line of text by the brain, as a step. Null: this is an ordinary message. */
export function toolStep(message: Pick<PotatoMessage, 'id' | 'text'>): ToolStep | null {
  const text = message.text.trim();
  const line = TOOL_LINE.exec(text);
  if (line) {
    const [, tool, status, rest = ''] = line;
    const group = toolGroup(tool);
    if (status === 'failed') {
      const needsYou = /\(needs you\)/i.test(rest);
      const code = rest.replace(/\(needs you\)/i, '').trim().split(/\s+/)[0] ?? '';
      return { id: message.id, tool, label: toolLabel(tool), summary: failureWords(code), ok: false, needsYou, details: code || null, group };
    }
    const saw = rest.replace(/^saw\s+/i, '').trim();
    if (!saw) return { id: message.id, tool, label: toolLabel(tool), summary: '', ok: true, needsYou: false, details: null, group };
    if (looksLikeJson(saw)) {
      return { id: message.id, tool, label: toolLabel(tool), summary: '', ok: true, needsYou: false, details: prettyJson(saw), group };
    }
    return { id: message.id, tool, label: toolLabel(tool), summary: shorten(saw, SUMMARY_MAX), ok: true, needsYou: false, details: null, group };
  }
  if (isRawJson(text)) {
    return { id: message.id, tool: 'result', label: THREAD_COPY.gotResult, summary: '', ok: true, needsYou: false, details: prettyJson(text), group: 'other' };
  }
  return null;
}

export function stepsWord(n: number): string {
  return n === 1 ? '1 step' : `${n} steps`;
}

/** The one line over a run of steps: "Used the computer · 3 steps", "Started a helper". */
export function activityTitle(steps: readonly ToolStep[]): string {
  const n = steps.length;
  const groups = new Set(steps.map((s) => s.group));
  const failed = steps.filter((s) => !s.ok).length;
  let head: string;
  if (groups.size === 1 && groups.has('computer')) head = `Used the computer · ${stepsWord(n)}`;
  else if (groups.size === 1 && groups.has('helper')) head = n === 1 ? 'Started a helper' : `Asked helpers · ${stepsWord(n)}`;
  else if (n === 1) head = steps[0].label;
  else head = `Used tools · ${stepsWord(n)}`;
  return failed > 0 ? `${head} · ${failed} failed` : head;
}

// ---- reply: quote into the composer -----------------------------------------------------------------

/** The window event the Reply button sends. The composer picks it up and puts the quote in its box. */
export const POTATO_QUOTE_EVENT = 'pm:potato-quote';

export interface PotatoQuoteDetail {
  botId: string;
  /** The text to quote, already prefixed with "> ". */
  quote: string;
}

const QUOTE_MAX = 600;

/** The text of a message as a "> " quote, long ones cut short, ending in a blank line so the answer starts below it. */
export function quoteText(text: string): string {
  const clean = text.trim();
  const cut = clean.length > QUOTE_MAX ? `${clean.slice(0, QUOTE_MAX).trimEnd()}…` : clean;
  return `${cut.split(/\r?\n/).map((line) => `> ${line}`.trimEnd()).join('\n')}\n\n`;
}

export function emitQuote(botId: string, text: string, target: Pick<Window, 'dispatchEvent'> = window): void {
  target.dispatchEvent(new CustomEvent<PotatoQuoteDetail>(POTATO_QUOTE_EVENT, { detail: { botId, quote: quoteText(text) } }));
}

/** A draft with a quote put in front of what is already typed. */
export function withQuote(draft: string, quote: string): string {
  return `${quote}${draft}`;
}
