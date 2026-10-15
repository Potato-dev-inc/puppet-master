// The words of the phone approval card and its queue, in one place (design 2.9). Sentence case, plain
// words, no raw ids. The risk words are the desktop card's (`LEVEL_WORD` in approval-risk.ts).
import type { AttentionItem } from './attention';

/** "Claude Code", or "The agent" for an item with no pane. */
export function agentName(item: Pick<AttentionItem, 'pane'>): string {
  return item.pane?.agent?.trim() || 'The agent';
}

/** 24-hour clock time, the way the lock screen and the PC print it: "09:41". */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, '0');
  return `${two(d.getHours())}:${two(d.getMinutes())}`;
}

/** "2 min", "1 h 4 min", "just now". */
export function formatAge(fromMs: number, nowMs: number): string {
  const minutes = Math.max(0, Math.floor((nowMs - fromMs) / 60_000));
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** "in 8 min", "in 45 s": how long a question still waits. */
export function formatWait(msLeft: number): string {
  const left = Math.max(0, msLeft);
  if (left < 60_000) return `${Math.max(1, Math.round(left / 1000))} s`;
  return `${Math.max(1, Math.round(left / 60_000))} min`;
}

export const COPY = {
  /** A new prompt is up (or none is): the old card says so and never answers the new one. */
  changed: 'This question changed. Look again.',
  gone: (agent: string) => `This question has gone. ${agent} moved on.`,
  alreadyPc: (label: string) => `Already answered on your PC: ${label}.`,
  alreadyYou: (label: string) => `Already answered by you: ${label}.`,
  alreadyPhone: (device: string, label: string) => `Already answered from ${device}: ${label}.`,
  /** "Allowed once · 09:41 · from this phone" */
  answered: (label: string, clock: string) => `${label} · ${clock} · from this phone`,
  failed: (agent: string) => `Couldn't send your answer to ${agent}. Nothing was typed.`,
  offline: "Can't reach your PC. Your answer wasn't sent.",
  lost: "Didn't hear back from your PC. Your answer may not have arrived.",
  sending: 'Sending',
  sendingToPc: 'Sending to your PC',
  stillSending: "Still sending. Your PC hasn't confirmed yet.",
  watchOnly: 'This phone can watch only. To answer here, allow it on your PC: Settings › Phone.',
  controlOnly: "This phone can't do that. Allow it on your PC: Settings › Phone.",
  provenanceScreen: 'Read from the screen',
  caution: 'Read this before you allow it. It may be hard to undo.',
  noPrompt: (agent: string) => ({
    title: `${agent} is waiting for you`,
    body: "Puppet Master can't read this question. Open the pane to answer it.",
  }),
  signIn: (agent: string) => ({
    title: `${agent} needs you on your PC.`,
    body: "It's showing a sign-in screen. Puppet Master won't answer it for you.",
  }),
  expired: (agent: string, clock: string) => `${agent} stopped waiting at ${clock}.`,
  queue: (n: number) => (n === 1 ? '1 more waiting' : `${n} more waiting`),
  empty: { title: 'Nothing needs you', body: 'Agents that stop to ask will show up here.' },
  dealtWith: 'That request has been dealt with.',
  showDetails: 'Show details',
  hideDetails: 'Hide details',
  openPane: 'Open pane',
  tryAgain: 'Try again',
  /** After "This question changed": show the card again with the PC's current list. */
  lookAgain: 'Look again',
} as const;

/** How long a fresh "Sending" stays plain, then turns to "Sending to your PC", then "Still sending". */
export const SENDING_PHASE_MS = { toPc: 1_500, still: 8_000, lost: 20_000 } as const;

export type SendingPhase = 'sending' | 'sending_pc' | 'still' | 'lost';

/** What the card says about an answer that was sent `elapsedMs` ago and has not come back. */
export function sendingPhase(elapsedMs: number): SendingPhase {
  if (elapsedMs >= SENDING_PHASE_MS.lost) return 'lost';
  if (elapsedMs >= SENDING_PHASE_MS.still) return 'still';
  if (elapsedMs >= SENDING_PHASE_MS.toPc) return 'sending_pc';
  return 'sending';
}

export function sendingText(phase: Exclude<SendingPhase, 'lost'>): string {
  return phase === 'sending' ? COPY.sending : phase === 'sending_pc' ? COPY.sendingToPc : COPY.stillSending;
}
