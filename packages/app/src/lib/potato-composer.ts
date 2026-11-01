// The message box at the bottom of a Potato Bot thread: the pure rules behind it (what blocks it, the `@` and `/`
// pickers, attachments and their limits, quotes). Nothing here draws or talks to the app; Composer.tsx does.
import type { PotatoBotView } from './potato';

/** The longest message the brain takes (spec section 2). */
export const MAX_MESSAGE = 4000;
/** The box grows with the text up to this many lines, then scrolls. */
export const MAX_LINES = 8;
/** One line of text in the box (13 px text at 1.5 line height, rounded up). */
export const LINE_PX = 20;

export const COMPOSER_COPY = {
  off: 'Potato Bot is switched off. Turn it on in Labs to write.',
  overLimit: "Today's limit is reached. You can write again tomorrow, or raise the limit in Settings.",
  retry: 'Retry',
  dismiss: 'Dismiss',
  attach: 'Attach a file',
  dropHere: 'Drop pictures or text files here',
  sendFailed: 'Could not send that.',
  noOthers: 'No other bots to mention yet.',
  tooManyFiles: (max: number) => `You can attach up to ${max} files.`,
  notAllowed: (name: string) => `${name}: only pictures (png, jpg, gif, webp) and text files can be attached.`,
  tooBig: (name: string, max: string) => `${name} is bigger than ${max}.`,
  tooBigTogether: (max: string) => `The files together are bigger than ${max}.`,
  empty: (name: string) => `${name} is empty.`,
  unreadable: (name: string) => `${name} could not be read.`,
  tooLongWithFiles: 'Shorten the message a little so the files fit.',
  persona: 'Say /persona fish or /persona potato.',
};

/** Why the box is off, or `null` when it takes text. A bot that is switched off or out of money today cannot be written to. */
export function composerBlock(bot: Pick<PotatoBotView, 'brainState'>): string | null {
  if (bot.brainState === 'off') return COMPOSER_COPY.off;
  if (bot.brainState === 'daily_limit') return COMPOSER_COPY.overLimit;
  return null;
}

/** The key under which this bot's unsent text is kept (the app-wide draft store, one entry per bot). */
export function potatoDraftKey(botId: string): string {
  return `potato:${botId}`;
}

// ---- attachments --------------------------------------------------------------------------------

export type AttachKind = 'image' | 'text';

/** What the page keeps for one attached file until it is sent. `data` is base64. */
export interface Attachment {
  id: string;
  name: string;
  kind: AttachKind;
  size: number;
  data: string;
  /** A data URL for the little picture on an image chip. */
  preview?: string;
}

/** The caps; the app enforces the same ones again (`potato/attachments.rs`). */
export const ATTACH_LIMITS = { files: 4, fileBytes: 2 * 1024 * 1024, totalBytes: 4 * 1024 * 1024 } as const;

const IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp']);
const IMAGE_EXT = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp']);
const TEXT_TYPES = new Set(['application/json', 'application/xml', 'application/x-yaml', 'application/yaml', 'application/toml', 'application/x-sh', 'application/javascript', 'application/typescript']);
const TEXT_EXT = new Set([
  'txt', 'md', 'markdown', 'json', 'csv', 'tsv', 'log', 'yaml', 'yml', 'toml', 'ini', 'xml', 'html', 'htm', 'css', 'js', 'mjs', 'cjs', 'jsx',
  'ts', 'tsx', 'py', 'rs', 'go', 'java', 'kt', 'c', 'h', 'cpp', 'hpp', 'cs', 'rb', 'php', 'sh', 'bat', 'ps1', 'sql', 'env', 'conf', 'cfg', 'gitignore',
]);

const extOf = (name: string): string => {
  const dot = name.lastIndexOf('.');
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : '';
};

/** Picture, text, or `null` (not allowed). The type the browser reports wins; the name's ending is the fallback. */
export function attachKind(file: { name: string; type: string }): AttachKind | null {
  const type = file.type.toLowerCase();
  if (IMAGE_TYPES.has(type)) return 'image';
  if (type.startsWith('image/')) return null; // svg, bmp, tiff: not kept
  if (type.startsWith('text/') || TEXT_TYPES.has(type)) return 'text';
  if (type !== '' && type !== 'application/octet-stream') return null;
  const ext = extOf(file.name);
  if (IMAGE_EXT.has(ext)) return 'image';
  if (TEXT_EXT.has(ext)) return 'text';
  return null;
}

/** `1.5 MB`, `12 KB`, `300 B`. */
export function sizeWord(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

export type AttachCheck = { ok: true; kind: AttachKind } | { ok: false; reason: string };

/** Whether one more file may join the ones already attached. */
export function checkAttach(current: readonly Pick<Attachment, 'size'>[], file: { name: string; type: string; size: number }): AttachCheck {
  if (current.length >= ATTACH_LIMITS.files) return { ok: false, reason: COMPOSER_COPY.tooManyFiles(ATTACH_LIMITS.files) };
  const kind = attachKind(file);
  if (!kind) return { ok: false, reason: COMPOSER_COPY.notAllowed(file.name || 'That file') };
  if (file.size === 0) return { ok: false, reason: COMPOSER_COPY.empty(file.name) };
  if (file.size > ATTACH_LIMITS.fileBytes) return { ok: false, reason: COMPOSER_COPY.tooBig(file.name, sizeWord(ATTACH_LIMITS.fileBytes)) };
  const total = current.reduce((sum, a) => sum + a.size, 0) + file.size;
  if (total > ATTACH_LIMITS.totalBytes) return { ok: false, reason: COMPOSER_COPY.tooBigTogether(sizeWord(ATTACH_LIMITS.totalBytes)) };
  return { ok: true, kind };
}

/** The room a message has for its words once files are attached (each file adds one line about itself). */
export function wordsRoom(fileCount: number): number {
  return MAX_MESSAGE - fileCount * 160;
}

// ---- the `@` and `/` pickers --------------------------------------------------------------------

/** Where a picker's word starts in the text, and what has been typed after the `@` or `/`. */
export interface Trigger {
  start: number;
  query: string;
}

/** An `@word` right before the caret, at the start or after a space or line break. */
export function mentionAt(text: string, caret: number): Trigger | null {
  const before = text.slice(0, caret);
  const at = before.lastIndexOf('@');
  if (at < 0) return null;
  if (at > 0 && !/\s/.test(before[at - 1])) return null;
  const query = before.slice(at + 1);
  if (/\s/.test(query) || query.length > 24) return null;
  return { start: at, query };
}

/** `/` at the very start of the message, still on its first line. */
export function slashAt(text: string, caret: number): Trigger | null {
  if (!text.startsWith('/')) return null;
  const before = text.slice(0, caret);
  if (before.includes('\n') || caret < 1) return null;
  return { start: 0, query: before.slice(1) };
}

export interface MentionTarget {
  id: string;
  name: string;
}

/** The other bots that match what was typed: names that start with it first, then names that contain it. */
export function filterMentions(bots: readonly MentionTarget[], query: string, selfId: string): MentionTarget[] {
  const q = query.toLowerCase();
  const others = bots.filter((b) => b.id !== selfId);
  const starts = others.filter((b) => b.name.toLowerCase().startsWith(q) || b.id.toLowerCase().startsWith(q));
  const contains = others.filter((b) => !starts.includes(b) && (b.name.toLowerCase().includes(q) || b.id.toLowerCase().includes(q)));
  return [...starts, ...contains];
}

/** The word a mention puts in the text: the name without spaces. */
export function mentionWord(bot: MentionTarget): string {
  return `@${bot.name.replace(/\s+/g, '')}`;
}

export type CommandName = 'persona' | 'help' | 'new' | 'computer';

export interface SlashItem {
  /** The whole command as it goes into the box. */
  text: string;
  name: CommandName;
  hint: string;
}

/** Which commands the page can run. `new` and `computer` need the page to pass them in. */
export interface CommandSupport {
  newBot?: boolean;
  computer?: boolean;
}

/** Every command the picker offers, in order. */
export function slashItems(support: CommandSupport = {}): SlashItem[] {
  const items: SlashItem[] = [
    { text: '/persona fish', name: 'persona', hint: 'Talk as Fish' },
    { text: '/persona potato', name: 'persona', hint: 'Talk as Potato' },
  ];
  if (support.newBot) items.unshift({ text: '/new', name: 'new', hint: 'Make a new bot' });
  if (support.computer) items.push({ text: '/computer', name: 'computer', hint: "Open this bot's computer" });
  items.push({ text: '/help', name: 'help', hint: 'What you can type here' });
  return items;
}

/** The commands that fit what has been typed (`/per` finds both persona lines). */
export function filterSlash(items: readonly SlashItem[], query: string): SlashItem[] {
  const q = query.toLowerCase();
  return items.filter((i) => i.text.slice(1).startsWith(q));
}

export interface ParsedCommand {
  name: CommandName;
  arg: string;
}

/** The message as a command, or `null` when it is a normal message. Only a command the page can run counts. */
export function parseCommand(text: string, support: CommandSupport = {}): ParsedCommand | null {
  const match = /^\/([a-z]+)(?:[ \t]+([^\n]*))?$/i.exec(text.trim());
  if (!match) return null;
  const name = match[1].toLowerCase();
  const arg = (match[2] ?? '').trim().toLowerCase();
  if (name === 'persona' || name === 'help') return { name, arg };
  if (name === 'new' && support.newBot) return { name, arg };
  if (name === 'computer' && support.computer) return { name, arg };
  return null;
}

/** The text with the picked word in place of what was typed after the trigger; the caret goes after it. */
export function applyPick(text: string, caret: number, trigger: Trigger, insert: string, space = true): { value: string; caret: number } {
  const after = text.slice(caret);
  const tail = space && !/^\s/.test(after) ? ' ' : '';
  const value = text.slice(0, trigger.start) + insert + tail + after;
  return { value, caret: trigger.start + insert.length + tail.length };
}

// ---- quotes -------------------------------------------------------------------------------------

/** `> line` for every line of the text, then a blank line to write below. */
export function quoteBlock(text: string): string {
  const lines = text.replace(/\r\n/g, '\n').trim().split('\n');
  return `${lines.map((l) => `> ${l}`.trimEnd()).join('\n')}\n\n`;
}

/** The quote added under whatever is already in the box. */
export function addQuote(draft: string, quoted: string): string {
  if (quoted.trim() === '') return draft;
  const block = quoteBlock(quoted);
  return draft.trim() === '' ? block : `${draft.replace(/\s+$/, '')}\n\n${block}`;
}

/** What the `pm:potato-quote` event carries: the text, or `{ text, botId? }`. A quote for another bot is not ours. */
export function quoteFromEvent(detail: unknown, botId: string): string | null {
  if (typeof detail === 'string') return detail;
  if (!detail || typeof detail !== 'object') return null;
  const { text, botId: target } = detail as { text?: unknown; botId?: unknown };
  if (typeof text !== 'string') return null;
  if (typeof target === 'string' && target !== botId) return null;
  return text;
}

/** The /help line: what can be typed here, for the commands this page can run. */
export function helpText(support: CommandSupport = {}): string {
  const commands = slashItems(support).map((i) => i.text).join(', ');
  return `Type @ to mention another bot. Commands: ${commands}. Attach pictures and text files with the paperclip, by pasting, or by dropping them here.`;
}
