// A question a helper pane waits on, shown in the Chat of the pane that started it. Pure, no React: it only
// says WHICH helpers' cards a pane shows; the card, its answer route and its stale guard are the Inbox's own.
import { liveParentId } from './pane-helpers';

interface PaneLike {
  info: { id: string; parent_pane_id?: string | null };
  status: string;
}

/** Ancestors of a pane, nearest first (its parent, then that pane's parent, ...). Loops and gone parents end it. */
export function ancestorIds(list: ReadonlyArray<PaneLike>, id: string): string[] {
  const out: string[] = [];
  const seen = new Set([id]);
  for (let at = liveParentId(list, id); at && !seen.has(at); at = liveParentId(list, at)) {
    out.push(at);
    seen.add(at);
  }
  return out;
}

/** The panes whose Chat shows this helper's question: its direct parent and its top-most ancestor (the same pane counts once). */
export function bubbleTargets(list: ReadonlyArray<PaneLike>, helperId: string): string[] {
  const chain = ancestorIds(list, helperId);
  if (chain.length === 0) return [];
  return [...new Set([chain[0], chain[chain.length - 1]])];
}

/**
 * The helpers whose question belongs in `paneId`'s Chat, in pane order. `asking(id)` says the helper waits on a
 * card right now (the same test the pane's own dock uses), so an answered or gone question drops out by itself.
 */
export function helpersAskingFor(list: ReadonlyArray<PaneLike>, paneId: string, asking: (id: string) => boolean): string[] {
  return list.filter((p) => p.info.id !== paneId && asking(p.info.id) && bubbleTargets(list, p.info.id).includes(paneId)).map((p) => p.info.id);
}

/** "Claude Code helper asks": the card's lead-in, with the helper's agent name. */
export function helperAsksLabel(agentName: string): string {
  return `${agentName} helper asks`;
}

interface ItemLike {
  id: string;
  state: string;
  pane: { id: string } | null;
}

/** The phone's side: from the PC's attention items, the open card of each helper in `helperIds` (one per helper, list order). */
export function helperItems<T extends ItemLike>(items: ReadonlyArray<T>, helperIds: ReadonlyArray<string>, isCard: (item: T) => boolean): T[] {
  return helperIds.flatMap((id) => {
    const item = items.find((i) => i.state === 'open' && i.pane?.id === id && isCard(i));
    return item ? [item] : [];
  });
}
