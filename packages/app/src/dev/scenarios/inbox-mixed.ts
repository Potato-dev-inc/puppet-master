import { EXAMPLE_PANES, PANE_CLAUDE, PANE_CODEX } from '../fixtures';
import { CHANGED_SCREENS_STATUS } from '../canary-fixtures';
import { LIMIT_CARD } from '../limit-fixtures';
import type { Question } from '../../lib/questions';
import type { HarnessScenario } from '../types';

// Every kind of Inbox card at once: a low-risk and a high-risk approval, a question, a consent, a
// budget pause, a usage limit, an error and a version note. They share one shape, one time format
// and one action row, and sit in one list (high risk first, then the oldest).
const now = Date.now();
const questions: Question[] = [
  {
    id: 'q-migration',
    agent: 'Claude Code',
    pane_id: PANE_CLAUDE,
    question: 'The migration drops the legacy users table. Run it against staging now?',
    context: 'Staging was last backed up 2 hours ago. 14 rows reference the table.',
    choices: [
      { id: 'run', label: 'Run migration' },
      { id: 'backup', label: 'Back up first' },
      { id: 'skip', label: 'Skip for now' },
    ],
    allow_free_text: true,
    urgency: 'normal',
    created_at_ms: now - 4 * 60_000,
    expires_at_ms: now + 9 * 60_000,
    state: 'pending',
  },
  {
    id: 'consent-take-over',
    agent: 'Claude Code',
    question: 'Allow Claude Code to control Codex?',
    context: 'It would type into Codex and answer its questions. It did not start this pane.',
    choices: [
      { id: 'allow_once', label: 'Allow once' },
      { id: 'always', label: 'Always for this pane' },
      { id: 'deny', label: 'Deny' },
    ],
    allow_free_text: false,
    urgency: 'high',
    created_at_ms: now - 40_000,
    expires_at_ms: now + 560_000,
    state: 'pending',
    consent: { action: 'take_over', requester: 'Claude Code', target: 'Codex', pane_id: PANE_CODEX },
  },
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
  LIMIT_CARD,
];

const s: HarnessScenario = {
  name: 'inbox-mixed',
  route: 'cockpit',
  // Claude Code waits at a low-risk prompt, Codex at a high-risk one, PowerShell hit an error.
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CODEX ? { ...p, status: 'waiting_input' as const } : p)),
  questions,
  canary: CHANGED_SCREENS_STATUS,
  after: async (dom) => {
    await dom.waitFor('[data-testid="canary-notice"]');
    await dom.sleep(400);
  },
};
export default s;
