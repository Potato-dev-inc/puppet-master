// Questions an agent asks the human through the `ask_user` MCP tool (see docs/features/ask-user.md).
// Shape mirrors the Rust `questions::Question` (snake_case, as served by the bridge).

export interface QuestionChoice {
  id: string;
  label: string;
}

/** What a consent card is about (Rust `questions::ConsentInfo`). Only Puppet Master raises these. */
export interface ConsentInfo {
  action: 'take_over' | 'answer_prompt';
  /** Who is asking, as a name ("Claude Code", "Another agent"). */
  requester: string;
  /** What it wants to control, as a name ("Codex"). The card prefers the pane's own name. */
  target: string;
  pane_id?: string;
  /** For `answer_prompt`: how risky the prompt is. */
  risk?: 'medium' | 'high';
}

/**
 * What a usage-limit card is about (Rust `questions::LimitInfo`). Only Puppet Master raises these, when an
 * agent ran out of usage (`usage_limit`) or when a reminder the person asked for comes due (`reset_reminder`).
 */
export interface LimitInfo {
  kind: 'usage_limit' | 'reset_reminder';
  /** The agent's name in the words the card uses ("Claude Code"). */
  agent_label: string;
  /** What ran out ("session limit"), when the screen said. */
  window?: string;
  /** When it can continue (epoch ms), when the screen printed a time this computer could read. */
  reset_at_ms?: number;
  /** The time as the screen printed it. */
  reset_text?: string;
  /** The agent will carry on by itself at the reset. */
  self_resume_armed?: boolean;
  /** A run is bound to the pane, so "Stop here" has something to stop. */
  has_run?: boolean;
}

export interface Question {
  id: string;
  asker?: string;
  /** Name the agent gave itself. */
  agent?: string;
  pane_id?: string;
  question: string;
  choices: QuestionChoice[];
  allow_free_text: boolean;
  context?: string;
  urgency: 'normal' | 'high';
  created_at_ms: number;
  expires_at_ms: number;
  state: 'pending' | 'answered' | 'cancelled' | 'timed_out';
  /** Set when this is a consent card (an agent asking to control a pane it did not start). */
  consent?: ConsentInfo;
  /** `budget` for the pause card of a run that used its budget (only Puppet Master raises one). */
  kind?: string;
  /** Set when this is a usage-limit card (an agent ran out of usage), not a question an agent asked. */
  limit?: LimitInfo;
}

/** The pause card of an agent that used its budget: "Add 10 turns" / "Stop here". */
export const isBudget = (q: Question): boolean => q.kind === 'budget';

/** A consent card, not a question an agent asked. */
export const isConsent = (q: Question): boolean => q.consent !== undefined;

/** A usage-limit card (or the reminder for one), not a question an agent asked. */
export const isLimit = (q: Question): boolean => q.limit !== undefined;

/** "Start on #42 with Codex?": the card that asks before an agent starts on a GitHub issue. */
export const isIssueStart = (q: Question): boolean => q.kind === 'issue_start';

/** "Draft PR ready for #42?": the card that asks before a branch is pushed and a draft PR is opened. */
export const isIssuePush = (q: Question): boolean => q.kind === 'issue_push';

/** Either issue card (Assign an issue). These are raised by Puppet Master only and answered by the person only. */
export const isIssueCard = (q: Question): boolean => isIssueStart(q) || isIssuePush(q);

/** "CI failed on #61 (lint, test). Ask Codex to fix it?": the card that asks before an agent reads a failed CI log. */
export const isCiFix = (q: Question): boolean => q.kind === 'ci_fix';

/** "Push the fix to #61?": the card that asks before a CI fix is pushed. */
export const isCiPush = (q: Question): boolean => q.kind === 'ci_push';

/** Either Red to Green card. Raised by Puppet Master only and answered by the person only. */
export const isCiCard = (q: Question): boolean => isCiFix(q) || isCiPush(q);

/** What the person typed or picked: exactly one of the two. */
export type QuestionAnswer = { choice_id: string } | { text: string };

/** "Question from Claude Code": the agent's own name, else the pane's name, else "an agent". */
export function askerName(q: Question, paneName?: string): string {
  return q.agent?.trim() || paneName || 'an agent';
}

/** Pending and not past its deadline (the app expires them too; this hides a stale card sooner). */
export function isLive(q: Question, now: number): boolean {
  return q.state === 'pending' && q.expires_at_ms > now;
}

/** "8 min", "under a minute", "2 h": a short, rounded-up time left. */
export function timeLeftText(ms: number): string {
  if (ms <= 60_000) return 'under a minute';
  const minutes = Math.ceil(ms / 60_000);
  if (minutes <= 90) return `${minutes} min`;
  return `${Math.round(minutes / 60)} h`;
}

/** "Claude Code stops waiting in 8 min": how long the asking agent will still wait for an answer. */
export function waitCaption(q: Question, now: number, paneName?: string): string {
  const who = q.agent?.trim() || paneName || 'The agent';
  const left = q.expires_at_ms - now;
  if (left <= 0) return `${who} has stopped waiting`;
  return `${who} stops waiting in ${timeLeftText(left)}`;
}
