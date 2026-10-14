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

/** Panes that have something waiting on them, for the sidebar's "Needs you" group. */
export function needsYouPaneIds(items: ReadonlyArray<NeedsYouItem>): Set<string> {
  const ids = new Set<string>();
  for (const item of items) if (item.paneId) ids.add(item.paneId);
  return ids;
}

export const itemsOfKind = (items: ReadonlyArray<NeedsYouItem>, kind: NeedsYouKind): NeedsYouItem[] =>
  items.filter((item) => item.kind === kind);
