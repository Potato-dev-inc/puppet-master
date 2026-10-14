import { PANE_CLAUDE, PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Setting A on: Claude Code wants to control the Codex pane (Allow once / Always for this pane / Deny), and
// another agent wants to answer a risky question in the Claude pane (Allow once / Deny).
const now = Date.now();
const s: HarnessScenario = {
  name: 'inbox-consent',
  route: 'cockpit',
  questions: [
    {
      id: 'consent-take-over',
      agent: 'Claude Code',
      question: 'Allow Claude Code to control Codex?',
      context: 'It would type into the pane and answer its questions. It did not start this pane.',
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
      id: 'consent-answer',
      agent: 'Another agent',
      question: 'Allow another agent to answer a question in Claude Code?',
      context: 'Delete a folder outside this project. It did not start this pane.',
      choices: [
        { id: 'allow_once', label: 'Allow once' },
        { id: 'deny', label: 'Deny' },
      ],
      allow_free_text: false,
      urgency: 'high',
      created_at_ms: now - 5_000,
      expires_at_ms: now + 595_000,
      state: 'pending',
      consent: { action: 'answer_prompt', requester: 'Another agent', target: 'Claude Code', pane_id: PANE_CLAUDE, risk: 'high' },
    },
  ],
};
export default s;
