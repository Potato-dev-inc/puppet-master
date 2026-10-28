// Usage-limit cards for the harness (Keep going). The shape is what `questions::LimitInfo` and
// `handoff::card::limit_card` produce; the words are theirs too (see `handoff/card_tests.rs`). The default card
// is the one with an offer: another agent can take over, so `Continue on Codex` comes first.
import type { Question } from '../lib/questions';
import { PANE_CLAUDE } from './fixtures';

const now = Date.now();
const MIN = 60_000;

const OFFER_TEXT = 'Codex gets a short brief and works in the same folder. Claude Code in this pane is stopped first.';

/** Claude Code ran out mid-run; its own wait is off (or unseen); Codex can take over. */
export const LIMIT_CARD: Question = {
  id: 'limit-claude',
  agent: 'Claude Code',
  pane_id: PANE_CLAUDE,
  question: 'Claude Code ran out of usage. It can continue at 14:05.',
  context: OFFER_TEXT,
  choices: [
    { id: 'continue:codex', label: 'Continue on Codex' },
    { id: 'remind', label: 'Remind me at 14:05' },
    { id: 'stop', label: 'Stop here' },
  ],
  allow_free_text: false,
  urgency: 'high',
  created_at_ms: now - 2 * MIN,
  expires_at_ms: now + 12 * 60 * MIN,
  state: 'pending',
  limit: {
    kind: 'usage_limit',
    agent_label: 'Claude Code',
    window: 'session limit',
    reset_at_ms: now + 25 * MIN,
    reset_text: '2:05pm (Asia/Taipei)',
    self_resume_armed: false,
    has_run: true,
  },
};

/** Its own "continue automatically at usage limit" wait is armed: the card says it will carry on by itself. */
export const LIMIT_CARD_SELF_RESUME: Question = {
  ...LIMIT_CARD,
  id: 'limit-claude-self',
  context: `Claude Code is set to continue on its own. Continuing on Codex stops that. ${OFFER_TEXT}`,
  limit: { ...LIMIT_CARD.limit!, self_resume_armed: true },
};

/** The screen did not print a reset time. */
export const LIMIT_CARD_NO_RESET: Question = {
  ...LIMIT_CARD,
  id: 'limit-claude-none',
  question: "Claude Code ran out of usage. It didn't say when it can continue.",
  context: OFFER_TEXT,
  choices: [
    { id: 'continue:codex', label: 'Continue on Codex' },
    { id: 'remind', label: 'Remind me in 1 hour' },
    { id: 'stop', label: 'Stop here' },
  ],
  limit: { kind: 'usage_limit', agent_label: 'Claude Code', self_resume_armed: false, has_run: true },
};

/** No other agent can take over now: the card says why and points to Settings. Remind and Stop stay. */
export const LIMIT_CARD_NO_OFFER: Question = {
  ...LIMIT_CARD,
  id: 'limit-claude-no-offer',
  context: 'No other agent can take over now: no other agent is set up in this folder. Pick one in Settings > Agents > Keep going.',
  choices: LIMIT_CARD.choices.filter((c) => !c.id.startsWith('continue:')),
};

/** The card a reminder raises when the time comes. */
export const LIMIT_REMINDER: Question = {
  ...LIMIT_CARD,
  id: 'limit-reminder',
  question: "Claude Code's usage limit should have reset by now.",
  context: 'You asked to be reminded. Open its terminal to carry on.',
  choices: [{ id: 'ok', label: 'Got it' }],
  urgency: 'normal',
  limit: { kind: 'reset_reminder', agent_label: 'Claude Code', window: 'session limit' },
};
