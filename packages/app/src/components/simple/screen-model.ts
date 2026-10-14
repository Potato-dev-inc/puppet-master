// When Simple mode shows the agent's screen by itself. The terminal is hidden behind Details, but
// hiding a screen PM cannot read would be worse than showing it (IDEAS-R8-PLAIN-MODE §4.4): so the
// screen appears whenever the agent waits for something PM cannot turn into a plain card.
import type { PanePrompt } from '../../lib/cockpit-api';
import { simpleApproval } from './approval-text';

export interface ScreenFacts {
  /** `running | waiting_input | idle | error | exited` (PaneStatus + exited). */
  status: string;
  prompt: PanePrompt | null | undefined;
}

/**
 * True when PM cannot read what the agent shows: it waits for a person (`waiting_input`) but there
 * is no approval PM understands (a card Simple can word, with at least one answer). That covers an
 * unknown screen, a folder-trust question and a menu. An `error` pane also shows its screen,
 * because the screen holds the reason.
 */
export function needsScreen({ status, prompt }: ScreenFacts): boolean {
  if (status === 'error') return true;
  if (status !== 'waiting_input') return false;
  // Readable = a card Simple can word (see approval-text.ts) and at least one answer to give.
  return !(prompt && simpleApproval(prompt) && prompt.choices.length > 0);
}

/**
 * Whether to send the project's task again. The folder-trust question parks the run before
 * anything is typed (`PANE_BLOCKED_BY_PROMPT`). Once she has answered it the agent is idle, and the
 * same words she approved with Start are sent once. Never while a question is open, never twice.
 */
export function shouldResendTask(facts: {
  status: string;
  errorCode: string | null | undefined;
  handle: string | null | undefined;
  task: string | null | undefined;
  alreadySent: boolean;
}): boolean {
  return facts.status === 'idle' && facts.errorCode === 'PANE_BLOCKED_BY_PROMPT' && !!facts.handle && !!facts.task && !facts.alreadySent;
}
