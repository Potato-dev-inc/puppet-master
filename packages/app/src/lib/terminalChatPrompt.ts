// A terminal chat's question and the person's answer to it (Tauri commands `terminal_chat_prompt` and
// `terminal_chat_answer`, `src-tauri/src/terminal_chat/prompt.rs`). Desktop only: there is no bridge route, MCP tool
// or phone path for these. No UI here: a card calls `readTerminalChatPrompt` and, from a click handler,
// `answerTerminalChatPrompt` with `gestureOf(event)`.
//
// Rules the card keeps: no option is pre-picked or highlighted as the default, nothing answers on a timer or in
// an effect, and a stale answer is a reason to read the question again, never to retry the old pick.

import { invoke } from '@tauri-apps/api/core';
import type { UserGesture } from './user-gesture';

export interface TerminalChatOption {
  /** What to send back to `answerTerminalChatPrompt`. */
  id: string;
  /** The agent's own words for the option. */
  name: string;
}

/** `trust`: a folder or startup question. `permission`: allow or deny a command or an edit. `question`: a menu read by its shape. */
export type TerminalChatPromptKind = 'trust' | 'permission' | 'question';

export interface TerminalChatPromptView {
  /** Changes with the question and with each new occurrence of it; send it back with the pick. */
  promptId: string;
  /** The question, redacted, in the agent's words. */
  text: string;
  options: TerminalChatOption[];
  kind: TerminalChatPromptKind;
  /** The agent profile's own kind (`workspace_trust`, `command_approval`, ...). */
  rawKind: string;
  /** A one-line summary of what saying yes would do, when the screen gave one. */
  summary?: string;
  /** How risky saying yes is, when assessed. */
  risk?: { level: string; headline: string; reasons?: string[] };
}

/** The codes `answerTerminalChatPrompt` can fail with that a card acts on. Others come from the answer route. */
export type TerminalChatAnswerCode =
  | 'TERMINAL_CHAT_PROMPT_STALE'
  | 'TERMINAL_CHAT_UNKNOWN_OPTION'
  | 'ANSWER_NEEDS_A_PERSON'
  | (string & {});

export class TerminalChatAnswerError extends Error {
  readonly code: TerminalChatAnswerCode;
  constructor(code: TerminalChatAnswerCode, message: string) {
    super(message);
    this.name = 'TerminalChatAnswerError';
    this.code = code;
  }
  /** The screen changed since the card was drawn: read the question again and ask again. Nothing was sent. */
  get isStale(): boolean {
    return this.code === 'TERMINAL_CHAT_PROMPT_STALE';
  }
}

/** The Tauri error string `CODE: message` as an error with a code. */
export function parseTerminalChatError(raw: unknown): TerminalChatAnswerError {
  const text = raw instanceof Error ? raw.message : String(raw ?? '');
  const match = /^([A-Z][A-Z0-9_]+): ([\s\S]*)$/.exec(text);
  return match ? new TerminalChatAnswerError(match[1], match[2]) : new TerminalChatAnswerError('UNKNOWN', text);
}

/** The question the pane shows now, or `null` when nothing recognisable is on the screen. */
export function readTerminalChatPrompt(pane: string): Promise<TerminalChatPromptView | null> {
  return invoke<TerminalChatPromptView | null>('terminal_chat_prompt', { pane });
}

/**
 * The person's pick. `gesture` is their click (`gestureOf(event)` in the click handler): without a real, fresh,
 * trusted one the backend sends nothing, not even a refusal. Rejects with a `TerminalChatAnswerError`.
 */
export async function answerTerminalChatPrompt(
  pane: string,
  promptId: string,
  optionId: string,
  gesture: UserGesture,
): Promise<void> {
  try {
    await invoke<void>('terminal_chat_answer', { pane, promptId, optionId, gesture });
  } catch (error) {
    throw parseTerminalChatError(error);
  }
}
