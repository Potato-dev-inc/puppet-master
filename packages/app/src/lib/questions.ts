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
}

/** The pause card of an agent that used its budget: "Add 10 turns" / "Stop here". */
export const isBudget = (q: Question): boolean => q.kind === 'budget';

/** A consent card, not a question an agent asked. */
export const isConsent = (q: Question): boolean => q.consent !== undefined;

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
