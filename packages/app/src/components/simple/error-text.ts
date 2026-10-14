// Problems in Simple mode: three short lines and one button (IDEAS-R8-PLAIN-MODE §4.6). Every one
// says whether her work is safe, never uses the word for a failure, and never shows a code.
import type { LoopSignal } from '../../lib/loop-guard';
import { F, T } from './copy';

export type ProblemButton = 'start_again' | 'show_me' | 'sign_in' | 'ask_fix' | 'none';

export interface Problem {
  text: string;
  button: ProblemButton;
}

/** The problem for a run that ended with `code` (the codes `explainError` knows; others are generic). */
export function problemForCode(code: string | null | undefined, agent: string): Problem {
  switch (code) {
    case 'AGENT_EXITED':
      return { text: F.agentStopped(agent), button: 'start_again' };
    case 'WORKSPACE_TRUST_DECLINED':
      return { text: F.trustDeclined(agent), button: 'start_again' };
    case 'AGENT_NOT_INSTALLED':
      return { text: F.agentMissing(agent), button: 'none' };
    case 'PANE_NOT_READY':
    case 'PROMPT_NOT_SUBMITTED':
      return { text: F.messageLost(agent), button: 'show_me' };
    default:
      return { text: T.somethingWrong, button: 'show_me' };
  }
}

/** The problem for a stuck-agent signal from the loop guard. */
export function problemForLoop(signal: Pick<LoopSignal, 'kind'>, agent: string): Problem {
  return signal.kind === 'repeated_error' || signal.kind === 'repeated_action'
    ? { text: F.agentLoop(agent), button: 'none' }
    : { text: F.agentSlow(agent), button: 'none' };
}

export const STRINGS: string[] = [
  ...['AGENT_EXITED', 'WORKSPACE_TRUST_DECLINED', 'AGENT_NOT_INSTALLED', 'PANE_NOT_READY', 'OTHER'].map((c) => problemForCode(c, 'Claude Code').text),
  problemForLoop({ kind: 'repeated_error' }, 'Claude Code').text,
  problemForLoop({ kind: 'stalled' }, 'Claude Code').text,
];
