// Questions an agent asks the human through the `ask_user` MCP tool (see docs/features/ask-user.md).
// Shape mirrors the Rust `questions::Question` (snake_case, as served by the bridge).

export interface QuestionChoice {
  id: string;
  label: string;
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
}

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
