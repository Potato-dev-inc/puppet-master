// "Why is it stuck?" sentences for an agent that stopped with an error (UX-PHASE1 12.1).
// Only the codes the backend raises today. The code never appears in the text (it goes in a tooltip);
// the agent's name comes from its label, so nothing here knows an agent.

export type ErrorFix = 'start_again' | 'install' | 'open_pane';

export interface ErrorExplanation {
  sentence: string;
  fix: ErrorFix;
  /** Label of the one fix button. */
  fixLabel: string;
}

const FIX_LABEL: Record<ErrorFix, string> = {
  start_again: 'Start again',
  install: 'Install',
  open_pane: 'Open pane',
};

function explanation(sentence: string, fix: ErrorFix): ErrorExplanation {
  return { sentence, fix, fixLabel: FIX_LABEL[fix] };
}

/** The sentence and the fix for a run error code, or null for a code with no sentence yet. */
export function explainError(code: string | null | undefined, agent: string): ErrorExplanation | null {
  switch (code) {
    case 'WORKSPACE_TRUST_DECLINED':
      return explanation(`${agent} closed because you chose Don't trust. The task wasn't sent.`, 'start_again');
    case 'AGENT_EXITED':
      return explanation(`${agent} stopped unexpectedly. Your files are as it left them.`, 'start_again');
    case 'AGENT_NOT_INSTALLED':
      return explanation(`${agent} isn't installed on this PC.`, 'install');
    case 'PANE_NOT_READY':
      return explanation(`${agent} didn't reach a ready screen, so Puppet Master didn't type anything.`, 'open_pane');
    case 'PROMPT_NOT_SUBMITTED':
      return explanation(`The task was typed but ${agent} didn't start on it.`, 'open_pane');
    default:
      return null;
  }
}

/** What the card says when the run gave no code we can explain. */
export function unknownErrorSentence(agent: string): string {
  return `Something went wrong and ${agent} stopped. Open it to see what happened.`;
}
