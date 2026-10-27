// Side question: ask about a pane's chat without writing into the pane. The chat view builds the last turns as plain
// text (`buildSideContext`); the backend (src-tauri side_question/) redacts and caps it, wraps it as data in a
// one-shot read-only prompt and runs it headless. Nothing is saved and the pane's agent never sees any of it.
import { invoke } from '@tauri-apps/api/core';
import type { AskResult } from './askbar';
import type { ChatTurn } from './sessionChatModel';

/** The transcript sent is at most this many characters (the backend cuts to the same number). */
export const SIDE_CONTEXT_LIMIT = 12_000;
/** One message or reply longer than this is cut, so one huge text cannot fill the whole context. */
const MAX_TEXT = 3_000;
const MAX_SUMMARY = 200;

/** The agent that will answer (the pane's own when it can run headless, else the default pick). */
export interface SideTarget {
  agent: string;
  label: string;
}

export interface SideQuestionApi {
  /** `null`: no installed agent can answer a headless read-only question. */
  target: (paneId: string) => Promise<SideTarget | null>;
  ask: (paneId: string, request: { ask_id: string; question: string; context: string }) => Promise<AskResult>;
}

export const tauriSideQuestionApi: SideQuestionApi = {
  target: (paneId) => invoke<SideTarget | null>('side_question_target', { paneId }),
  ask: (paneId, request) => invoke<AskResult>('side_question_ask', { paneId, request }),
};

function cut(text: string, max: number): string {
  const chars = Array.from(text.trim());
  return chars.length <= max ? chars.join('') : `${chars.slice(0, max).join('')}…`;
}

/** One turn as lines: the person's message, then the agent's text and a short line per tool call, in order. */
export function turnLines(turn: ChatTurn): string[] {
  const lines: string[] = [];
  if (turn.user?.text.trim()) lines.push(`Person: ${cut(turn.user.text, MAX_TEXT)}`);
  for (const step of turn.steps) {
    if (step.kind === 'text') {
      if (step.item.text.trim()) lines.push(`Agent: ${cut(step.item.text, MAX_TEXT)}`);
    } else if (step.kind === 'tool') {
      const outcome = step.status === 'error' ? ' (failed)' : step.status === 'running' ? ' (running)' : '';
      lines.push(`Tool: ${cut(step.call.summary || step.call.name, MAX_SUMMARY)}${outcome}`);
    }
  }
  return lines;
}

/**
 * The last turns of a chat as plain text for the side question: oldest first, newest last, at most `limit`
 * characters. Whole turns are dropped from the old end (a note says how many); when even the newest turn is too long
 * its end is kept. Tool output is not included, only each call's one-line summary. Secrets are masked by the backend.
 */
export function buildSideContext(turns: readonly ChatTurn[], limit = SIDE_CONTEXT_LIMIT): string {
  const blocks = turns.map((turn) => turnLines(turn).join('\n')).filter((block) => block !== '');
  const kept: string[] = [];
  let size = 0;
  for (let i = blocks.length - 1; i >= 0; i--) {
    const add = Array.from(blocks[i]).length + (kept.length > 0 ? 2 : 0);
    if (size + add > limit) {
      if (kept.length === 0) kept.unshift(Array.from(blocks[i]).slice(-limit).join(''));
      break;
    }
    kept.unshift(blocks[i]);
    size += add;
  }
  const dropped = blocks.length - kept.length;
  const note = dropped > 0 ? `[${dropped} earlier ${dropped === 1 ? 'turn' : 'turns'} left out]\n\n` : '';
  return `${note}${kept.join('\n\n')}`;
}
