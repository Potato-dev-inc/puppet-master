// What the phone approval card shows, as a pure function of the item the PC listed, what this phone
// may do and what this phone has sent. No React here, so every state is tested on its own
// (approval-card.test.ts) and drawn by components/mobile/ApprovalCard.tsx.
import type { AttentionChoice, AttentionItem } from './attention';
import { LEVEL_WORD } from './approval-risk';
import { COPY, agentName, formatAge, formatClock, sendingPhase, sendingText } from './attention-copy';

/** The dot glyph: `(+)` a person is needed, `(~)` happening, `(v)` done, `(:)` gone, `(x)` failed. */
export type CardGlyph = 'waiting_input' | 'running' | 'exited' | 'idle' | 'error';

/** Which answer this phone has in flight or got back for one item. */
export type LocalAnswer =
  | { phase: 'idle' }
  | { phase: 'sending'; choiceId: string; label: string; startedAt: number }
  /** The request failed on the way: `offline` = it never reached the PC, `lost` = no reply in time. */
  | { phase: 'failed'; why: 'offline' | 'lost' | 'server'; choiceId: string; label: string }
  | { phase: 'answered'; label: string; at: number }
  | { phase: 'changed' }
  | { phase: 'gone' }
  | { phase: 'already'; by: 'pc' | 'you' | 'phone'; deviceName?: string; label?: string };

export const IDLE: LocalAnswer = { phase: 'idle' };

export type CardState =
  | 'open'
  | 'watch_only'
  | 'sending'
  | 'lost'
  | 'offline'
  | 'failed'
  | 'answered'
  | 'already'
  | 'changed'
  | 'gone'
  | 'expired'
  | 'no_prompt'
  | 'signin';

export interface CardButton {
  id: string;
  label: string;
  role: 'primary' | 'secondary';
  /** Gets the keyboard focus when the card opens (Deny on a high-risk prompt, never Allow). */
  focus: boolean;
  disabled: boolean;
}

export interface CardView {
  state: CardState;
  glyph: CardGlyph;
  /** The pane's name, top left. */
  heading: string;
  /** The agent and how long it has been waiting, top right. */
  meta: string;
  /** What is being asked: the risk headline, "Trust this folder?", the agent's sentence. */
  title: string;
  /** `Low risk` / `Check first` / `High risk`, when the prompt was rated. */
  riskWord: string | null;
  riskLevel: 'low' | 'medium' | 'high' | null;
  /** The command or file, as the PC read it. */
  summary: string | null;
  /** Plain sentences behind the risk, up to six (inside "Show details"). */
  reasons: string[];
  caution: string | null;
  /** The line under the title for every state that is not the plain open card. */
  status: string | null;
  buttons: CardButton[];
  /** "Try again" is offered. */
  retry: boolean;
  /** "Open pane" is offered. */
  openPane: boolean;
  details: boolean;
  provenance: string | null;
  /** The card is final (answered, gone, ...): it carries no buttons and leaves the list soon. */
  settled: boolean;
}

interface Options {
  /** This phone holds the `answer` ability. */
  canAnswer: boolean;
  local: LocalAnswer;
  now: number;
}

const isDeny = (c: AttentionChoice) => /^(deny|no|reject|cancel|decline|quit|stop|don'?t)\b/i.test(c.label.trim()) || /^(deny|no|reject|cancel|decline|quit|stop|dont)\b/i.test(c.id.replace(/[_-]+/g, ' ').trim());

/** The buttons: the PC's layout decides what leads. Broad answers never come from a phone. */
export function buttonsFor(item: AttentionItem, disabled: boolean): CardButton[] {
  const prompt = item.prompt;
  const choices = (prompt?.choices ?? []).filter((c) => !c.broad && !(prompt?.layout?.more ?? []).includes(c.id));
  const layout = prompt?.layout;
  const primary = layout?.primary ?? choices[0]?.id ?? null;
  const ordered = [...choices].sort((a, b) => Number(b.id === primary) - Number(a.id === primary));
  return ordered.map((c) => ({
    id: c.id,
    label: c.label,
    role: c.id === primary ? 'primary' : 'secondary',
    focus: layout?.focus === c.id,
    disabled,
  }));
}

function baseView(item: AttentionItem, now: number): CardView {
  const agent = agentName(item);
  const level = item.risk?.level ?? null;
  const isTrust = item.kind === 'trust_gate';
  const age = formatAge(item.since_ms ?? now, now);
  return {
    state: 'open',
    glyph: 'waiting_input',
    heading: item.pane?.heading ?? agent,
    meta: `${item.pane?.agent ?? agent} · ${age}`,
    title: isTrust ? item.title : (item.risk?.headline?.trim() || item.title),
    riskWord: item.risk ? (item.risk.word || LEVEL_WORD[item.risk.level]) : null,
    riskLevel: level,
    summary: isTrust ? item.sentence : (item.prompt?.summary ?? item.sentence),
    reasons: (item.risk?.reasons ?? []).slice(0, 6),
    caution: level === 'high' ? COPY.caution : null,
    status: null,
    buttons: [],
    retry: false,
    openPane: false,
    details: true,
    provenance: item.prompt ? COPY.provenanceScreen : null,
    settled: false,
  };
}

function settledView(view: CardView, state: CardState, glyph: CardGlyph, status: string, openPane = false): CardView {
  return { ...view, state, glyph, status, buttons: [], retry: false, openPane, details: false, caution: null, settled: true };
}

/** The card for one item. */
export function cardView(item: AttentionItem, { canAnswer, local, now }: Options): CardView {
  const view = baseView(item, now);
  const agent = agentName(item);
  const res = item.resolution;

  // The list says it is over: that wins over whatever the request is doing.
  if (item.state === 'resolved') {
    const label = res?.choice_label ?? (local.phase === 'sending' || local.phase === 'failed' ? local.label : undefined) ?? 'Answered';
    if (res?.state === 'answered') {
      const fromHere = local.phase === 'sending' || local.phase === 'answered' || local.phase === 'failed';
      if (fromHere) return settledView(view, 'answered', 'exited', COPY.answered(label, formatClock(res.at_ms)));
      if (res.by === 'desktop') return settledView(view, 'already', 'exited', COPY.alreadyPc(label));
      return settledView(view, 'already', 'exited', COPY.alreadyPhone(res.device_name?.trim() || 'another phone', label));
    }
    if (res?.state === 'expired') return settledView(view, 'expired', 'idle', COPY.expired(agent, formatClock(res.at_ms)), true);
    return settledView(view, 'gone', 'idle', COPY.gone(agent), true);
  }

  switch (local.phase) {
    case 'answered':
      return settledView(view, 'answered', 'exited', COPY.answered(local.label, formatClock(local.at)));
    case 'already': {
      const label = local.label ?? 'Answered';
      const text = local.by === 'pc' ? COPY.alreadyPc(label) : local.by === 'you' ? COPY.alreadyYou(label) : COPY.alreadyPhone(local.deviceName?.trim() || 'another phone', label);
      return settledView(view, 'already', 'exited', text);
    }
    case 'changed':
      return { ...view, state: 'changed', glyph: 'waiting_input', status: COPY.changed, buttons: [], openPane: true, details: false, caution: null };
    case 'gone':
      return settledView(view, 'gone', 'idle', COPY.gone(agent), true);
    default:
  }

  const prompt = item.prompt;
  if (item.kind === 'trust_gate' && prompt && (prompt.choices ?? []).length === 0) {
    const copy = COPY.signIn(agent);
    return { ...view, state: 'signin', title: copy.title, summary: null, status: copy.body, openPane: true, details: false, provenance: null };
  }
  if (!prompt || !prompt.choices || prompt.choices.length === 0 || !prompt.fingerprint) {
    const copy = COPY.noPrompt(agent);
    return { ...view, state: 'no_prompt', title: copy.title, riskWord: null, riskLevel: null, summary: null, reasons: [], caution: null, status: copy.body, openPane: true, details: false, provenance: null };
  }
  if (!canAnswer) {
    return { ...view, state: 'watch_only', status: COPY.watchOnly, buttons: [] };
  }

  if (local.phase === 'sending') {
    const phase = sendingPhase(now - local.startedAt);
    const buttons = buttonsFor(item, true);
    if (phase === 'lost') {
      return { ...view, state: 'lost', glyph: 'error', status: COPY.lost, buttons: [], retry: true, openPane: true, details: false, caution: null };
    }
    return { ...view, state: 'sending', glyph: 'running', status: sendingText(phase), buttons, details: false };
  }
  if (local.phase === 'failed') {
    const why = local.why;
    const text = why === 'offline' ? COPY.offline : why === 'lost' ? COPY.lost : COPY.failed(agent);
    // Offline before the tap went anywhere: the card keeps its glyph; a lost or refused answer is a failure.
    const glyph: CardGlyph = why === 'offline' ? 'waiting_input' : 'error';
    return { ...view, state: why === 'offline' ? 'offline' : why === 'lost' ? 'lost' : 'failed', glyph, status: text, buttons: [], retry: true, openPane: why !== 'offline', details: false, caution: null };
  }

  return { ...view, state: 'open', buttons: buttonsFor(item, false) };
}

/** True for the choices an answer may send: the ones the card shows. */
export function isOfferedChoice(item: AttentionItem, choiceId: string): boolean {
  return buttonsFor(item, false).some((b) => b.id === choiceId);
}

/** Whether a choice looks like "no", for the screen reader's hint. */
export function isDenyLabel(choice: AttentionChoice): boolean {
  return isDeny(choice);
}
