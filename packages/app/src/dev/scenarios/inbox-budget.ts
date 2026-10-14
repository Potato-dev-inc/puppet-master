import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// An agent used its 20 turns: the run is paused before its next turn and the Inbox says what to do.
const now = Date.now();
const s: HarnessScenario = {
  name: 'inbox-budget',
  route: 'cockpit',
  questions: [
    {
      id: 'budget-1',
      agent: 'Claude Code',
      pane_id: PANE_CLAUDE,
      question: 'Claude Code used its 20 turns.',
      context: "20 of 20 turns. It's paused after its last turn. Nothing was lost.",
      choices: [
        { id: 'add_turns', label: 'Add 10 turns' },
        { id: 'stop', label: 'Stop here' },
      ],
      allow_free_text: false,
      urgency: 'normal',
      created_at_ms: now - 20_000,
      expires_at_ms: now + 86_000_000,
      state: 'pending',
      kind: 'budget',
    },
  ],
};
export default s;
