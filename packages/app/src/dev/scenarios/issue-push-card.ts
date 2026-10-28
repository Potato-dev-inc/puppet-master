import type { Question } from '../../lib/questions';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Assign an issue, before a push: the copy changed and a draft PR is ready. The first line is the facts, the
// next is a reason. Look first opens the run's pane. The title is made up.
const now = Date.now();
export const PUSH_CARD: Question = {
  id: 'issue-push-42',
  kind: 'issue_push',
  pane_id: PANE_CODEX,
  question: 'Draft PR ready for #42?',
  context: [
    '3 files · +40 −5 · Codex · 12 turns',
    'Changes a CI file: .github/workflows/test.yml',
  ].join('\n'),
  choices: [
    { id: 'push', label: 'Open draft PR' },
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
  name: 'issue-push-card',
  route: 'cockpit',
  questions: [PUSH_CARD],
};
export default s;
