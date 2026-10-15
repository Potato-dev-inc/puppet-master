// One "Needs you" model. Every surface that announces attention (the sidebar group, the Inbox and
// its badge, the big Doto counter, the phone counter) renders a view of the list derived here, so
// they can never disagree about how many things need the person.
//
// Pure and import-light on purpose: the phone app imports it too.

export type NeedsYouKind =
  /** A pane is stopped at a permission prompt. */
  | 'approval'
  /** An agent asked something through `ask_user`. */
  | 'question'
  /** A pane's process hit an error. */
  | 'error'
  /** The loop guard thinks a pane is stuck. */
  | 'stuck'
  /** Reserved for a decision gate in a plan; nothing produces it yet. */
  | 'gate'
  /** An Ask-bar answer that arrived while the bar was hidden. */
  | 'answer'
  /**
   * An agent wants to control a pane it did not start, or answer a risky prompt in one. Counts toward
   * the number, sorts with high risk, and never has a lock-screen action (UX-PHASE1 section 0.4).
   */
  | 'consent'
  /** An agent used its budget and is paused before its next turn. Counts; never a lock-screen action. */
  | 'budget'
  /**
   * An agent ran out of usage (or a reminder the person asked for is due). Counts toward the number; the
   * Inbox card offers "Remind me" and "Stop here" (detect-only hand-off, `src-tauri/src/handoff`).
   */
  | 'limit'
  /**
   * A mission waits on a decision: the tests were changed, it is not getting anywhere, or it cannot go
   * on (`src-tauri/src/missions/cards.rs`, key `mission:<id>`). Its budget card is a question already.
   */
  | 'mission';

export interface NeedsYouItem {
  /** Stable across renders: `<kind>:<pane or question or notice id>`. */
  id: string;
  kind: NeedsYouKind;
  paneId?: string;
  /** Plain risk of the prompt, when one was read from the screen (approvals only). */
  risk?: 'low' | 'medium' | 'high';
  /** Epoch ms since when it has been waiting, when known. */
  since?: number;
}

export interface AttentionInput {
  panes: ReadonlyArray<{ id: string; status: string }>;
  /** Detected prompt per pane (desktop only): supplies the risk of an approval. */
  prompts?: Readonly<Record<string, { risk?: { level: string } | null } | null | undefined>>;
  /** Live (pending, not expired) questions. */
  questions?: ReadonlyArray<{ id: string; pane_id?: string; created_at_ms?: number; consent?: unknown; kind?: string; limit?: unknown }>;
  /** Panes the loop guard flagged. Unknown panes are ignored. */
  stuckPaneIds?: ReadonlyArray<string>;
  /** Ask-bar answers nobody saw yet. */
  notices?: ReadonlyArray<{ id: string; at_ms?: number }>;
  /** When each pane entered its current status. */
  statusSince?: ReadonlyMap<string, number>;
  /** Missions (Labs): one item per mission whose card waits on the person. */
  missions?: ReadonlyArray<{ id: string; state: string; needs?: string | null }>;
}

const RISKS = new Set(['low', 'medium', 'high']);

/** Everything that needs the person right now, one entry per thing. */
export function deriveNeedsYou(input: AttentionInput): NeedsYouItem[] {
  const items: NeedsYouItem[] = [];
  const known = new Set(input.panes.map((p) => p.id));

  for (const pane of input.panes) {
    if (pane.status === 'waiting_input') {
      const level = input.prompts?.[pane.id]?.risk?.level;
      items.push({
        id: `approval:${pane.id}`,
        kind: 'approval',
        paneId: pane.id,
        risk: level && RISKS.has(level) ? (level as NeedsYouItem['risk']) : undefined,
        since: input.statusSince?.get(pane.id),
      });
    }
  }
  for (const q of input.questions ?? []) {
    const paneId = q.pane_id && known.has(q.pane_id) ? q.pane_id : undefined;
    // A consent card is not a question the agent asked: it is a decision about the agent.
    if (q.consent) items.push({ id: `consent:${q.id}`, kind: 'consent', risk: 'high', since: q.created_at_ms });
    else if (q.kind === 'budget') items.push({ id: `budget:${q.id}`, kind: 'budget', paneId, since: q.created_at_ms });
    else if (q.limit) items.push({ id: `limit:${q.id}`, kind: 'limit', paneId, since: q.created_at_ms });
    else items.push({ id: `question:${q.id}`, kind: 'question', paneId, since: q.created_at_ms });
  }
  for (const pane of input.panes) {
    if (pane.status === 'error') {
      items.push({ id: `error:${pane.id}`, kind: 'error', paneId: pane.id, since: input.statusSince?.get(pane.id) });
    }
  }
  for (const id of input.stuckPaneIds ?? []) {
    if (known.has(id)) items.push({ id: `stuck:${id}`, kind: 'stuck', paneId: id });
  }
  for (const n of input.notices ?? []) {
    items.push({ id: `answer:${n.id}`, kind: 'answer', since: n.at_ms });
  }
  // The same rule as Rust's mission cards: a mission that needs you, except for its budget card
  // (that is a budget question, counted above). One item per mission, keyed like the Rust store.
  for (const m of input.missions ?? []) {
    if (m.state === 'needs_you' && m.needs !== 'budget') items.push({ id: `mission:${m.id}`, kind: 'mission' });
  }
  return items;
}

/**
 * Ask-bar answers wait in "Ready for you": nothing is blocked, so they carry no number. Every other
 * kind blocks an agent until a person acts. (The Rust side owns the lanes: `attention::model::lane_of`.)
 */
export const isBlockingKind = (kind: NeedsYouKind): boolean => kind !== 'answer';

/** Lower wins: a pane shows one dialog at a time (the Rust store applies the same rule). */
const PANE_STATE_PRECEDENCE: Partial<Record<NeedsYouKind, number>> = { approval: 1, error: 2, stuck: 3 };

/**
 * The items that count: the length of this is the number every surface shows. It is the Rust
 * store's rule applied to the derived list: Ask-bar answers wait in Ready for you, one id counts
 * once, and a pane has ONE pane-state item (approval beats error beats stuck), so an errored pane
 * that the loop guard also flags is one problem, not two.
 */
export function blockingItems(items: ReadonlyArray<NeedsYouItem>): NeedsYouItem[] {
  const seen = new Set<string>();
  const unique = items.filter((i) => isBlockingKind(i.kind) && !seen.has(i.id) && seen.add(i.id));
  const best = new Map<string, number>();
  for (const item of unique) {
    const rank = PANE_STATE_PRECEDENCE[item.kind];
    if (rank !== undefined && item.paneId) best.set(item.paneId, Math.min(best.get(item.paneId) ?? rank, rank));
  }
  return unique.filter((item) => {
    const rank = PANE_STATE_PRECEDENCE[item.kind];
    return rank === undefined || !item.paneId || best.get(item.paneId) === rank;
  });
}

/** Panes that have something waiting on them, for the sidebar's "Needs you" group. */
export function needsYouPaneIds(items: ReadonlyArray<NeedsYouItem>): Set<string> {
  const ids = new Set<string>();
  for (const item of items) if (item.paneId) ids.add(item.paneId);
  return ids;
}

export const itemsOfKind = (items: ReadonlyArray<NeedsYouItem>, kind: NeedsYouKind): NeedsYouItem[] =>
  items.filter((item) => item.kind === kind);

// ---------------------------------------------------------------------------------------------
// The Rust snapshot (src-tauri/src/attention). `GET /attention`, the `attention` SSE frame and the
// Tauri event `attention://changed` all carry this shape. Clients apply a frame only when its `rev`
// is greater than the last one they applied; a reconnect re-reads `GET /attention`.

export type AttentionKind = NeedsYouKind | 'trust_gate' | 'tamper' | 'budget' | 'limit' | 'setup' | 'review' | 'done';

/** One thing the prompt on screen offers (`broad` = a "don't ask again" answer). */
export interface AttentionChoice {
  id: string;
  label: string;
  broad: boolean;
}

/** What the card showed, as the PC read it from the screen. */
export interface AttentionPrompt {
  prompt_id: string;
  /** How many times this pane has asked; the same command asked again is a new occurrence. */
  occurrence: number;
  kind: string;
  summary: string | null;
  /** Absent from an older PC: the card then has no buttons and offers the pane instead. */
  choices?: AttentionChoice[];
  /** Sent back with an answer; the PC refuses the answer when the live screen no longer matches. */
  fingerprint?: string;
  /** Which choice leads, which has focus and which sit behind "More options" (decided in Rust). */
  layout?: { primary: string | null; focus: string | null; more: string[] };
  provenance?: 'screen';
}

export interface AttentionResolution {
  /** `answered | gone | expired | withdrawn` */
  state: string;
  /** `desktop | phone` */
  by: string | null;
  choice_label: string | null;
  reason: string | null;
  at_ms: number;
  /** The phone that answered ("Anna's iPhone"). */
  device_name?: string;
}

export interface AttentionItem {
  /** Opaque and stable for one occurrence. Never render it. */
  id: string;
  /** `<kind>:<subject>`; the same string `deriveNeedsYou` calls the id. */
  key: string;
  kind: AttentionKind;
  lane: 'blocking' | 'ready';
  rank: number;
  state: 'open' | 'resolved';
  since_ms: number | null;
  expires_at_ms: number | null;
  pane: { id: string; heading: string; agent: string | null; project: string | null } | null;
  title: string;
  sentence: string | null;
  risk: { level: 'low' | 'medium' | 'high'; word: string; headline: string | null; reasons?: string[] } | null;
  prompt: AttentionPrompt | null;
  actions: { id: string; label: string; style: string; needs: string }[];
  resolution: AttentionResolution | null;
}

export interface AttentionSnapshot {
  v: number;
  rev: number;
  counts: { blocking: number; ready: number };
  items: AttentionItem[];
  /** Only on `GET /attention` for a paired device: what that device may do. */
  device?: { can_answer: boolean; can_type: boolean; can_control: boolean };
}

/** Keep the newer of two snapshots: a frame is applied only when its `rev` is greater. */
export function newerSnapshot(current: AttentionSnapshot | null, next: AttentionSnapshot): AttentionSnapshot {
  // No snapshot at all (a backend without the command answers nothing, the dev harness among them):
  // keep what we had. Reading `next.device` on it took the whole desktop window down.
  if (!next) return (current ?? next) as AttentionSnapshot;
  if (current && current.rev > next.rev) return current;
  // The same revision read again: nothing changed on the list, but what this phone may do might have.
  if (current && current.rev === next.rev) return next.device ? { ...current, device: next.device } : current;
  // The `device` block is only in `GET /attention` (the broadcast frame is shared by every phone), so a
  // frame keeps what the last read said this phone may do.
  return next.device || !current?.device ? next : { ...next, device: current.device };
}

/** The number every surface shows: open blocking items. */
export const blockingCount = (snapshot: AttentionSnapshot): number => snapshot.counts.blocking;

/** Keys of the open items that block (what the old derivation calls ids). */
export function blockingKeys(snapshot: AttentionSnapshot): string[] {
  return snapshot.items.filter((i) => i.state === 'open' && i.lane === 'blocking').map((i) => i.key);
}

/**
 * Differences between the front-end derivation and the Rust snapshot, as plain sentences; empty when
 * they agree. Used by the dev-only shadow check so a disagreement shows up as a console warning.
 */
export function attentionMismatch(items: ReadonlyArray<NeedsYouItem>, snapshot: AttentionSnapshot, isBlocking: (kind: NeedsYouKind) => boolean = () => true): string[] {
  const local = new Set(items.filter((i) => isBlocking(i.kind)).map((i) => i.id));
  const rust = new Set(blockingKeys(snapshot));
  const out: string[] = [];
  for (const id of local) if (!rust.has(id)) out.push(`only the front end has ${id}`);
  for (const id of rust) if (!local.has(id)) out.push(`only Rust has ${id}`);
  return out;
}

// ---------------------------------------------------------------------------------------------
// Answering from the phone card: `POST /attention/{id}/answer`.

/** What the phone sends: the choice and exactly what the card showed. */
export interface AttentionAnswerBody {
  choice?: string;
  /** Free text, for a question that takes it. */
  text?: string;
  prompt_id?: string;
  occurrence?: number;
  fingerprint?: string;
  /** Made once per tap and re-sent by "Try again": a retry never presses twice. */
  answer_key?: string;
}

/** What the PC says happened. HTTP 200 for all of these; a refusal is an `AttentionAnswerError`. */
export type AttentionAnswerOutcome =
  | { outcome: 'answered'; choice_label: string; at_ms: number }
  | { outcome: 'already_answered'; by: 'pc' | 'you' | 'phone'; device_name?: string | null; choice_label?: string | null }
  /** A different prompt is up on that pane now. Nothing was typed. */
  | { outcome: 'changed' }
  /** No prompt is up any more. Nothing was typed. */
  | { outcome: 'gone'; reason?: string }
  /** The key could not be written. Nothing was typed. */
  | { outcome: 'failed'; code?: string; detail?: string };

/** The PC refused the request (403 not allowed, 400 bad request). */
export class AttentionAnswerError extends Error {
  constructor(
    readonly status: number,
    readonly body: { error?: string; code?: string; cap?: string; hint?: string },
  ) {
    super(`answer refused (${status}): ${body.code ?? body.error ?? 'unknown'}`);
    this.name = 'AttentionAnswerError';
  }
}
