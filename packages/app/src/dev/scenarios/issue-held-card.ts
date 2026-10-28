import type { Question } from '../../lib/questions';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Assign an issue, held: the git settings changed while the agent worked, so there is no "Open draft PR"
// choice at all. The person can only look at the copy or throw it away.
const now = Date.now();
export const HELD_CARD: Question = {
  id: 'issue-held-42',
  kind: 'issue_push',
  pane_id: PANE_CODEX,
  question: 'Draft PR ready for #42?',
  context: [
    '3 files · +40 −5 · Codex · 12 turns',
    'The git settings of the copy changed during the run.',
  ].join('\n'),
  choices: [
    { id: 'look', label: 'Look first' },
    { id: 'throw_away', label: 'Throw away' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: now - 300_000,
  expires_at_ms: now + 86_000_000,
  state: 'pending',
};

const s: HarnessScenario = {
  name: 'issue-held-card',
  route: 'cockpit',
  questions: [HELD_CARD],
};
export default s;
