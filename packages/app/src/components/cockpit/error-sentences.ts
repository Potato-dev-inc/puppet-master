// "Why did it stop?" for an agent that stopped with an error (UX-PHASE1 12.1, audit P1-13).
// Every card says three things: what happened, what is safe, and one next step. The code never
// appears in the text (it goes in a tooltip); the agent's name comes from its label, so nothing
// here knows an agent.

export type ErrorFix = 'start_again' | 'install' | 'open_pane';

export interface ErrorExplanation {
  /** The card's title: what happened, verb first. */
  title: string;
  /** What is safe and what to do, in one or two short sentences. */
  sentence: string;
  fix: ErrorFix;
  /** Label of the one fix button. */
  fixLabel: string;
  /**
   * `error` is the (x) glyph: something broke. `notice` is the calm one: the person chose this
   * (they declined a folder), so it is not drawn as a failure.
   */
  glyph: 'error' | 'notice';
}

const FIX_LABEL: Record<ErrorFix, string> = {
  start_again: 'Start again',
  install: 'Install',
  open_pane: 'Open',
};

function explanation(title: string, sentence: string, fix: ErrorFix, glyph: ErrorExplanation['glyph'] = 'error'): ErrorExplanation {
  return { title, sentence, fix, fixLabel: FIX_LABEL[fix], glyph };
}

/** The title, sentence and fix for a run error code, or null for a code with no sentence yet. */
export function explainError(code: string | null | undefined, agent: string): ErrorExplanation | null {
  switch (code) {
    case 'WORKSPACE_TRUST_DECLINED':
      // Starting again asks about the folder again; Puppet Master never answers that question for you.
      return explanation(
        `${agent} didn't start`,
        "You chose not to trust this folder, so the task wasn't sent. Start again to be asked once more.",
        'start_again',
        'notice',
      );
    case 'AGENT_EXITED':
      return explanation(`${agent} stopped`, 'It closed unexpectedly. Your files are as it left them.', 'start_again');
    case 'AGENT_NOT_INSTALLED':
      return explanation(`${agent} isn't installed`, "It isn't on this PC yet, so nothing ran.", 'install');
    case 'PANE_NOT_READY':
      return explanation(`${agent} wasn't ready`, "It didn't reach a ready screen, so Puppet Master typed nothing.", 'open_pane');
    case 'PROMPT_NOT_SUBMITTED':
      return explanation(`${agent} didn't start the task`, "The task was typed, but it didn't start on it.", 'open_pane');
    default:
      return null;
  }
}

/** The title and sentence when the run gave no code we can explain. */
export function unknownError(agent: string): { title: string; sentence: string } {
  return { title: `${agent} stopped`, sentence: 'Your files are as it left them. Open it to see what happened.' };
}
