// Potato Bot's first run (spec P5): a new bot (no messages yet) opens its thread with a scripted first message in its
// persona, and a choice card under it. Nothing here calls the brain: the words are fixed. A click sends the chosen text
// as the person's first message through the normal send path. The card is gone after the first real message and is
// never shown again for that bot (a local marker, so clearing the history does not bring it back).
import type { PotatoBotView, PotatoMessage, TalkPersona } from './potato';

export interface FirstRunScript {
  /** The first line. */
  greeting: string;
  /** What the bot can do, one line each. The computer line is there only when the sandbox is on. */
  points: string[];
  /** The line above the choice card. */
  question: string;
}

export interface FirstRunChoice {
  id: 'chat' | 'project' | 'computer' | 'what';
  /** The button's words. */
  label: string;
  /** The text sent as the person's first message. */
  text: string;
  /** Needs the sandbox Labs switch: left out while it is off. */
  needsComputer?: boolean;
}

export const FIRST_RUN_CHOICES: readonly FirstRunChoice[] = [
  { id: 'chat', label: 'Just chat', text: "Let's just chat." },
  { id: 'project', label: 'Help with a project', text: 'I want help with a project.' },
  { id: 'computer', label: 'Use the computer', text: 'Use your computer to help me.', needsComputer: true },
  { id: 'what', label: 'What can you do?', text: 'What can you do?' },
];

/** The choices the person sees: "Use the computer" only when the sandbox is on. */
export function visibleChoices(computerOn: boolean): FirstRunChoice[] {
  return FIRST_RUN_CHOICES.filter((c) => !c.needsComputer || computerOn);
}

interface Voice {
  greeting: (name: string) => string;
  chat: string;
  project: string;
  computer: string;
  question: string;
}

// Short, friendly, simple English. Fish is a little lazy and shy; Potato is cheerful and warm (the persona packs in
// potato-brain/persona/packs/*/core.md).
const VOICES: Record<TalkPersona, Voice> = {
  fish: {
    greeting: (name) => `Hi, I'm ${name}. I'm a little lazy, but I can still help. Here is what I can do:`,
    chat: 'Chat with me about anything.',
    project: 'Send a helper agent to your project folder. It asks you first.',
    computer: 'Use my own computer (a sandbox), if you turn it on.',
    question: 'So, what do you want to do first?',
  },
  potato: {
    greeting: (name) => `Hi, my friend! I'm ${name}, a little potato king. Here is what I can do:`,
    chat: 'Chat with me about anything. Hehe.',
    project: 'Send a helper to your project folder. I always ask you first.',
    computer: 'Use my own computer (a sandbox), if you turn it on.',
    question: 'What would you like to do first?',
  },
};

/** The scripted first message for a bot's persona. The computer line is left out while the sandbox is off. */
export function firstRunScript(persona: TalkPersona, name: string, computerOn: boolean): FirstRunScript {
  const voice = VOICES[persona] ?? VOICES.fish;
  const points = [voice.chat, voice.project, ...(computerOn ? [voice.computer] : [])];
  return { greeting: voice.greeting(name), points, question: voice.question };
}

/** A bot is new until it has any message: no last-message time yet and no rows in its thread. */
export function isNewBot(bot: Pick<PotatoBotView, 'lastAt'>, messages: readonly PotatoMessage[]): boolean {
  return bot.lastAt === null && messages.length === 0;
}

const MARK = '1';
const keyFor = (botId: string) => `pm.potato.firstrun.${botId}`;

/** The browser's store, or null when there is none (no window, or site data blocked). */
function store(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/** Whether this bot's first run is over for good. A broken store reads as not over. */
export function firstRunDone(botId: string): boolean {
  try {
    return store()?.getItem(keyFor(botId)) === MARK;
  } catch {
    return false;
  }
}

/** Remembers that this bot has been used, so its first run never comes back. A convenience only. */
export function markFirstRunDone(botId: string): void {
  try {
    store()?.setItem(keyFor(botId), MARK);
  } catch {
    // Not remembered: the card may show once more after a restart, and the messages still decide.
  }
}
