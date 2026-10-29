import type { Question } from '../../lib/questions';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Red to Green, the push card: the agent made a fix and Puppet Master checked it. The first line is the facts, the
// next lines are the reasons. Nothing is pushed until the person says so. The names are made up.
const now = Date.now();
export const PUSH_CARD: Question = {
  id: 'ci-push-61',
  kind: 'ci_push',
  pane_id: PANE_CODEX,
  question: 'Push the fix to #61?',
  context: [
    'Repo: acme/billing (pull request #61)',
    '1 file · +1 −1 · 1 commit · Codex · 4 turns',
    'The check log had 2 secrets hidden.',
  ].join('\n'),
  choices: [
    { id: 'push', label: 'Push fix' },
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
  name: 'ci-push-card',
  route: 'cockpit',
  questions: [PUSH_CARD],
};
export default s;
