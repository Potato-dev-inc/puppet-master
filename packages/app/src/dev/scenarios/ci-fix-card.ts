import type { Question } from '../../lib/questions';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Red to Green, the fix card: CI failed on a pull request Puppet Master opened. The agent may read the cleaned log
// only after the person says yes. The check names, the repo and the numbers are made up.
const now = Date.now();
export const FIX_CARD: Question & { fix_id: string } = {
  id: 'ci-fix-61',
  kind: 'ci_fix',
  pane_id: PANE_CODEX,
  question: 'CI failed on #61 (lint, test). Ask Codex to fix it?',
  context: [
    'Repo: acme/billing (pull request #61)',
    'Try 1 of 2 · It continues in its pane',
    'Log: 200 lines · 2 secrets hidden',
  ].join('\n'),
  choices: [
    { id: 'fix', label: 'Ask Codex to fix' },
    { id: 'not_now', label: 'Not now' },
    { id: 'stop_watching', label: 'Stop watching this PR' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: now - 120_000,
  expires_at_ms: now + 86_000_000,
  state: 'pending',
  fix_id: 'cf-0123456789ab',
};

const s: HarnessScenario = {
  name: 'ci-fix-card',
  route: 'cockpit',
  questions: [FIX_CARD],
};
export default s;
