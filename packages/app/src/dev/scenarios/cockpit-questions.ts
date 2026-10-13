import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Two questions agents asked through ask_user: one with choices (from a known pane), one free text.
const now = Date.now();
const s: HarnessScenario = {
  name: 'cockpit-questions',
  route: 'cockpit',
  questions: [
    {
      id: 'q-choices',
      pane_id: PANE_CLAUDE,
      question: 'The migration drops the legacy users table. Run it against staging now?',
      context: 'Staging was last backed up 2 hours ago. 14 rows reference the table.',
      choices: [
        { id: 'run', label: 'Run migration' },
        { id: 'backup', label: 'Back up first' },
        { id: 'skip', label: 'Skip for now' },
      ],
      allow_free_text: false,
      urgency: 'high',
      created_at_ms: now - 95_000,
      expires_at_ms: now + 500_000,
      state: 'pending',
    },
    {
      id: 'q-text',
      agent: 'Planner',
      question: 'Which branch should the release notes be generated from?',
      choices: [],
      allow_free_text: true,
      urgency: 'normal',
      created_at_ms: now - 20_000,
      expires_at_ms: now + 580_000,
      state: 'pending',
    },
  ],
};
export default s;
