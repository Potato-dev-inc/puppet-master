// When Simple mode shows the agent's screen by itself. The terminal is hidden behind Details, but
// hiding a screen PM cannot read would be worse than showing it (IDEAS-R8-PLAIN-MODE §4.4): so the
// screen appears whenever the agent waits for something PM cannot turn into a plain card.
import type { PanePrompt } from '../../lib/cockpit-api';

export interface ScreenFacts {
  /** `running | waiting_input | idle | error | exited` (PaneStatus + exited). */
  status: string;
  prompt: PanePrompt | null | undefined;
}

/**
 * True when PM cannot read what the agent shows: it waits for a person (`waiting_input`) but there
 * is no approval PM understands (a card with a risk and at least one answer). That covers an
 * unknown screen, a folder-trust question and a menu. An `error` pane also shows its screen,
 * because the screen holds the reason.
 */
export function needsScreen({ status, prompt }: ScreenFacts): boolean {
  if (status === 'error') return true;
  if (status !== 'waiting_input') return false;
  return !(prompt && prompt.risk && prompt.choices.length > 0);
}
