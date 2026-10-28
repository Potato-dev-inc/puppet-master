import type { Question } from '../../lib/questions';
import type { HarnessScenario } from '../types';

// Assign an issue, before a start: a GitHub issue that someone on the list labeled. The second author is not on
// the list, so the card says why it asks first. The title and text are made up.
const now = Date.now();
export const START_CARD: Question = {
  id: 'issue-start-42',
  kind: 'issue_start',
  question: 'Start on #42 "Login button does nothing on Safari" with Codex?',
  context: [
    'Repo: acme/billing (issue #42)',
    'Labeled by @you. Written by @someone.',
    "Asks first because: Written by someone who isn't on your list",
    'The agent reads the whole text. Start of it:',
    '{"title":"Login button does nothing on Safari","body":"When I click Log in nothing happens. The console shows a timeout."}',
  ].join('\n'),
  choices: [
    { id: 'start', label: 'Start with Codex' },
    { id: 'not_now', label: 'Not now' },
    { id: 'ignore', label: 'Ignore this issue' },
  ],
  allow_free_text: false,
  urgency: 'normal',
  created_at_ms: now - 120_000,
  expires_at_ms: now + 86_000_000,
  state: 'pending',
};

const s: HarnessScenario = {
  name: 'issue-start-card',
  route: 'cockpit',
  questions: [START_CARD],
};
export default s;
