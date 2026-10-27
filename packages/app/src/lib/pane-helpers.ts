// Helper panes nested under the pane of the agent that started them (Puppet Master's run_agent,
// delegate_work, spawn_agent). The backend records `parent_pane_id` once, display-only; this file
// turns the flat pane list into the sidebar's tree. Pure, no React.

interface PaneLike {
  info: { id: string; parent_pane_id?: string | null };
  status: string;
}

export interface HelperNode<T extends PaneLike> {
  pane: T;
  /** Direct helpers, in list order. Empty for a row promoted into Needs you. */
  helpers: HelperNode<T>[];
  /** The parent's pane, set on a row shown away from its parent (a helper that needs you). */
  awayFrom?: T;
}

export interface HelperGroups<T extends PaneLike> {
  needs: HelperNode<T>[];
  working: HelperNode<T>[];
  idle: HelperNode<T>[];
}

type GroupKey = keyof HelperGroups<PaneLike>;

/** Needs you = waiting_input + error (or an open question), Working = running, Idle = the rest. */
function groupOf(pane: PaneLike, questionPanes?: ReadonlySet<string>): GroupKey {
  if (pane.status === 'waiting_input' || pane.status === 'error' || questionPanes?.has(pane.info.id)) return 'needs';
  return pane.status === 'running' ? 'working' : 'idle';
}

function parentIn<T extends PaneLike>(byId: ReadonlyMap<string, T>, id: string): string | null {
  const parent = byId.get(id)?.info.parent_pane_id;
  if (!parent || parent === id || !byId.has(parent)) return null;
  // A malformed loop (a is b's parent and b is a's): its panes stay at top level.
  const seen = new Set([id]);
  for (let at: string | null | undefined = parent; at && byId.has(at); at = byId.get(at)?.info.parent_pane_id) {
    if (seen.has(at)) return null;
    seen.add(at);
  }
  return parent;
}

/** The live pane a helper belongs under: its recorded parent when that pane is still listed. */
export function liveParentId(list: ReadonlyArray<PaneLike>, id: string): string | null {
  return parentIn(new Map(list.map((p) => [p.info.id, p] as const)), id);
}

/**
 * The sidebar tree. A pane with a live parent is nested under it (whatever its own status), so a
 * parent's row is followed by its helpers. A pane whose parent is gone is top level. Status groups
 * hold top-level panes by their own status; a helper that needs you is also listed in Needs you,
 * away from its parent, unless its parent's tree already sits in that group.
 */
export function groupHelperTree<T extends PaneLike>(list: ReadonlyArray<T>, questionPanes?: ReadonlySet<string>): HelperGroups<T> {
  const byId = new Map(list.map((p) => [p.info.id, p] as const));
  const children = new Map<string, T[]>();
  const roots: T[] = [];
  for (const pane of list) {
    const parent = parentIn(byId, pane.info.id);
    if (parent) children.set(parent, [...(children.get(parent) ?? []), pane]);
    else roots.push(pane);
  }
  const nodeOf = (pane: T, trail: ReadonlySet<string>): HelperNode<T> => ({
    pane,
    helpers: (children.get(pane.info.id) ?? []).filter((c) => !trail.has(c.info.id)).map((c) => nodeOf(c, new Set([...trail, pane.info.id]))),
  });
  const groups: HelperGroups<T> = { needs: [], working: [], idle: [] };
  for (const root of roots) {
    const node = nodeOf(root, new Set());
    const home = groupOf(root, questionPanes);
    groups[home].push(node);
    if (home === 'needs') continue;
    // Somewhere under a row in Working or Idle: attention must not hide behind a parent or a fold.
    for (const helper of descendants(node)) {
      if (groupOf(helper.pane, questionPanes) !== 'needs') continue;
      const parentId = parentIn(byId, helper.pane.info.id);
      groups.needs.push({ pane: helper.pane, helpers: [], awayFrom: parentId ? byId.get(parentId) : undefined });
    }
  }
  return groups;
}

/** Every helper under `node`, nearest first. */
export function descendants<T extends PaneLike>(node: HelperNode<T>): HelperNode<T>[] {
  const out: HelperNode<T>[] = [];
  const queue = [...node.helpers];
  while (queue.length) {
    const next = queue.shift()!;
    out.push(next);
    queue.push(...next.helpers);
  }
  return out;
}

/** "1 helper", "2 helpers": every pane under this row. */
export function helperChipText(node: HelperNode<PaneLike>): string {
  const n = descendants(node).length;
  return `${n} ${n === 1 ? 'helper' : 'helpers'}`;
}

/** The pane header's line for a helper: "Helper of <title>", or null when it has no live parent. */
export function helperOfLabel<T extends PaneLike>(
  list: ReadonlyArray<T>,
  id: string,
  titleOf: (parent: T) => string,
): { parentId: string; text: string } | null {
  const parentId = liveParentId(list, id);
  const parent = parentId ? list.find((p) => p.info.id === parentId) : undefined;
  return parent && parentId ? { parentId, text: `Helper of ${titleOf(parent)}` } : null;
}
