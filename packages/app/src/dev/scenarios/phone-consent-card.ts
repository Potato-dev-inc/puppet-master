import { PANE_CODEX } from '../fixtures';
import { QUEUE_PANES } from '../attention-fixtures';
import type { AttentionItem } from '../../lib/attention';
import type { Question } from '../../lib/questions';
import type { HarnessScenario } from '../types';

const now = Date.now();

// Phone: another agent wants to control a pane it did not start. A decision about an agent: Deny,
// Allow once, no pre-selected default. It sorts with high risk, ahead of calmer things.
const question: Question = {
  id: 'consent-1',
  agent: 'Claude Code',
  question: 'Allow Claude Code to control Codex?',
  context: 'It would type into the pane and answer its questions. It did not start this pane.',
  choices: [
    { id: 'deny', label: 'Deny' },
    { id: 'allow_once', label: 'Allow once' },
  ],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: now - 40_000,
  expires_at_ms: now + 560_000,
  state: 'pending',
  consent: { action: 'take_over', requester: 'Claude Code', target: 'Codex', pane_id: PANE_CODEX },
};

const item: AttentionItem = {
  id: 'C0nsentX',
  key: 'consent:consent-1',
  kind: 'consent',
  lane: 'blocking',
  rank: 0,
  state: 'open',
  since_ms: now - 40_000,
  expires_at_ms: now + 560_000,
  pane: null,
  title: 'Claude Code wants to control another agent',
  sentence: null,
  risk: null,
  prompt: null,
  actions: [],
  resolution: null,
};

const s: HarnessScenario = {
  name: 'phone-consent-card',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    digest: false,
    questionList: [question],
    attention: { blocking: 1, items: [item] },
  },
  after: async (dom) => {
    await dom.waitFor('[data-testid="needs-you-queue"]');
    await dom.sleep(700);
  },
};
export default s;
