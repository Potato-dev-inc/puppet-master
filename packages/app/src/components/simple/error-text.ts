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

/**
 * The one problem to show for a project, or `null`. A pane in an error state, a stuck-agent
 * signal, or a run that failed while the agent is idle (a message that never reached it). A run
 * parked on the folder-trust question is not a problem: its task is sent again by itself.
 */
export function currentProblem(facts: {
  status: string;
  runStatus: string | null | undefined;
  runCode: string | null | undefined;
  loop: Pick<LoopSignal, 'kind'> | null | undefined;
  agent: string;
}): Problem | null {
  if (facts.status === 'error') return problemForCode(facts.runCode, facts.agent);
  if (facts.loop) return problemForLoop(facts.loop, facts.agent);
  if (facts.runStatus === 'failed' && facts.runCode && facts.runCode !== 'PANE_BLOCKED_BY_PROMPT' && facts.status !== 'running') {
    return problemForCode(facts.runCode, facts.agent);
  }
  return null;
}

export const STRINGS: string[] = [
  ...['AGENT_EXITED', 'WORKSPACE_TRUST_DECLINED', 'AGENT_NOT_INSTALLED', 'PANE_NOT_READY', 'OTHER'].map((c) => problemForCode(c, 'Claude Code').text),
  problemForLoop({ kind: 'repeated_error' }, 'Claude Code').text,
  problemForLoop({ kind: 'stalled' }, 'Claude Code').text,
];
