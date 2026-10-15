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

/** What a button says, in Puppet Master's words and not the agent's own menu text. */
export interface ChoiceWords {
  text: string;
  /** A command or scope named inside the words ("npm test"), shown as inline code, never in backticks. */
  code?: string;
}

const HOTKEY_HINT = /\s*\((?:[a-z]{1,10}(?:\+[a-z]{1,10})*|esc)\)\s*$/i;
const ASK_AGAIN_FOR = /\b(?:do(?:n'?t| not)|never)\s+ask\s+again\s+(?:for|on)\s+(.+)$/i;
const STARTS_WITH = /\b(?:start|starts|starting)\s+with\s+(.+)$/i;
const SESSION = /\b(?:this|the)\s+session\b|\bduring\s+this\b/i;

const cleanLabel = (label: string): string => label.replace(HOTKEY_HINT, '').trim();
const unquote = (text: string): string => text.replace(HOTKEY_HINT, '').replace(/^[`'"‘“]+|[`'"’”.]+$/g, '').trim();

/**
 * The same words on every approval, whatever the agent's menu says: "Allow once", "Always allow
 * <command>" and "Deny". Only the first plain yes and the first no are renamed. A choice that does
 * not fit (a second yes, a scope this does not name) keeps its own label, so no meaning is lost.
 */
/**
 * A startup gate (a folder-trust dialog: Claude's and Codex's `workspace_trust`): a decision about a
 * folder, not about a command. Its own words ("Trust this folder", "Don't trust") stay; "Allow once"
 * would be wrong (the answer is saved by the agent for good).
 */
export const isStartupGate = (kind: string | null | undefined): boolean => /trust/i.test(kind ?? '');

export function choiceWords(choices: ReadonlyArray<PromptChoice>): Map<string, ChoiceWords> {
  const out = new Map<string, ChoiceWords>();
  let yes = false;
  let no = false;
  for (const choice of choices) {
    const label = cleanLabel(choice.label);
    if (isDenyChoice(choice)) {
      out.set(choice.id, no ? { text: label } : { text: 'Deny' });
      no = true;
    } else if (isBroadChoice(choice)) {
      const forWhat = ASK_AGAIN_FOR.exec(label)?.[1];
      const prefix = forWhat ? STARTS_WITH.exec(forWhat)?.[1] : undefined;
      if (prefix && unquote(prefix)) out.set(choice.id, { text: 'Always allow commands starting with', code: unquote(prefix) });
      else if (forWhat && unquote(forWhat)) out.set(choice.id, { text: 'Always allow', code: unquote(forWhat) });
      else if (SESSION.test(label)) out.set(choice.id, { text: 'Allow for this session' });
      else out.set(choice.id, { text: label });
    } else {
      out.set(choice.id, yes ? { text: label } : { text: 'Allow once' });
      yes = true;
    }
  }
  return out;
}

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
 * High-risk prompts hide the "always allow" family behind More options, put Deny first, never make
 * Allow the primary or the focused button, and make Deny both when there is one. Nothing is
 * removed: every choice stays reachable.
 */
export function layoutChoices(prompt: Pick<PanePrompt, 'choices' | 'risk'>): ChoiceLayout {
  const choices = prompt.choices;
  if (prompt.risk?.level !== 'high') {
    return { visible: choices, more: [], focusId: null, primaryId: choices[0]?.id ?? null };
  }
  const more = choices.filter(isBroadChoice);
  const visible = choices.filter((c) => !isBroadChoice(c));
  const deny = visible.find(isDenyChoice) ?? null;
  // Deny leads, so the safe answer is the first thing the eye and the Tab key meet.
  const ordered = deny ? [deny, ...visible.filter((c) => c !== deny)] : visible;
  return { visible: ordered, more, focusId: deny?.id ?? null, primaryId: deny?.id ?? null };
}

export function riskGlyph(level: RiskLevel): 'ring' | 'triangle' | 'stop' {
  return level === 'low' ? 'ring' : level === 'medium' ? 'triangle' : 'stop';
}

export function firstReason(risk: PromptRisk): string {
  return risk.reasons[0] ?? '';
}
