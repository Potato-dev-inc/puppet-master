// Pure helpers behind the approval card: which answers are "don't ask again", and what a
// high-risk prompt demotes. Kept free of React so the rules are testable on their own.
import type { PanePrompt, PromptChoice, PromptRisk, RiskLevel } from './cockpit-api';

const BROAD = /\b(always|allow all|approve all|all commands|all tools|don'?t ask|do not ask|never ask|every time|for this session|remember|yes to all|forever|auto[- ]?(approve|accept))\b|\ball\b/i;
const DENY = /^(deny|no|reject|cancel|decline|quit|stop|don'?t)\b/i;

/** The backend flags broad answers; for builds that do not, fall back to the wording. */
export function isBroadChoice(choice: PromptChoice): boolean {
  if (typeof choice.broad === 'boolean') return choice.broad;
  return BROAD.test(`${choice.id.replace(/[_-]+/g, ' ')} ${choice.label}`);
}

export function isDenyChoice(choice: PromptChoice): boolean {
  return DENY.test(choice.label.trim()) || DENY.test(choice.id.replace(/[_-]+/g, ' ').trim());
}

export const LEVEL_WORD: Record<RiskLevel, string> = {
  low: 'Low risk',
  medium: 'Check first',
  high: 'High risk',
};

export const CAUTION = 'Read this before you allow it. It may be hard to undo.';

export interface ChoiceLayout {
  /** Always visible. */
  visible: PromptChoice[];
  /** Behind "More options" (high risk only: the "don't ask again" family). */
  more: PromptChoice[];
  /** Which visible choice gets the keyboard focus; `null` = the details button, never Allow. */
  focusId: string | null;
  /** Which visible choice looks like the primary action. */
  primaryId: string | null;
}

/**
 * Normal prompts keep every choice in view and the first one primary (the agent's own order).
 * High-risk prompts hide the "always allow" family behind More options, never make Allow the
 * primary or the focused button, and make Deny both when there is one. Nothing is removed: every
 * choice stays reachable.
 */
export function layoutChoices(prompt: Pick<PanePrompt, 'choices' | 'risk'>): ChoiceLayout {
  const choices = prompt.choices;
  if (prompt.risk?.level !== 'high') {
    return { visible: choices, more: [], focusId: null, primaryId: choices[0]?.id ?? null };
  }
  const more = choices.filter(isBroadChoice);
  const visible = choices.filter((c) => !isBroadChoice(c));
  const deny = visible.find(isDenyChoice) ?? null;
  return { visible, more, focusId: deny?.id ?? null, primaryId: deny?.id ?? null };
}

export function riskGlyph(level: RiskLevel): 'ring' | 'triangle' | 'stop' {
  return level === 'low' ? 'ring' : level === 'medium' ? 'triangle' : 'stop';
}

export function firstReason(risk: PromptRisk): string {
  return risk.reasons[0] ?? '';
}
