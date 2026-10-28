import type { Question } from '../../lib/questions';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Red to Green, held: the fix edited a CI workflow file, so the backend sends no "Push fix" choice at all.
// The person can only look at the copy or decline. The names are made up.
const now = Date.now();
export const HELD_CARD: Question = {
  id: 'ci-held-61',
  kind: 'ci_push',
  pane_id: PANE_CODEX,
  question: 'Push the fix to #61?',
  context: [
    'Repo: acme/billing (pull request #61)',
    '1 file · +1 −1 · 1 commit · Codex · 4 turns',
    'Changes a CI file: .github/workflows/ci.yml',
  ].join('\n'),
  choices: [
    { id: 'look', label: 'Look first' },
    { id: 'dont_push', label: "Don't push" },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: now - 300_000,
  expires_at_ms: now + 86_000_000,
  state: 'pending',
};

const s: HarnessScenario = {
  name: 'ci-held-card',
  route: 'cockpit',
  questions: [HELD_CARD],
};
export default s;
