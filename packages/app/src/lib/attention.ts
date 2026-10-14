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
  | 'consent';

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
  questions?: ReadonlyArray<{ id: string; pane_id?: string; created_at_ms?: number; consent?: unknown }>;
  /** Panes the loop guard flagged. Unknown panes are ignored. */
  stuckPaneIds?: ReadonlyArray<string>;
  /** Ask-bar answers nobody saw yet. */
  notices?: ReadonlyArray<{ id: string; at_ms?: number }>;
  /** When each pane entered its current status. */
  statusSince?: ReadonlyMap<string, number>;
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
  risk: { level: 'low' | 'medium' | 'high'; word: string; headline: string | null } | null;
  prompt: { prompt_id: string; occurrence: number; kind: string; summary: string | null } | null;
  actions: { id: string; label: string; style: string; needs: string }[];
  resolution: { state: string; by: string | null; choice_label: string | null; reason: string | null; at_ms: number } | null;
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
  return current && current.rev >= next.rev ? current : next;
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
