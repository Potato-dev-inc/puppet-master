// The helpers a pane's agent started through Puppet Master (run_agent, delegate_work, spawn_agent), as compact rows
// in that pane's chat ("Helper: <task> ... working"). They come from the pane list (`parent_pane_id`), not from the
// session file, and are placed in the turn during which they were started. Pure, no React.
import type { ChatTurn } from './sessionChatModel';
import { liveParentId } from './pane-helpers';

export type ChatHelperStatus = 'working' | 'needs-you' | 'done' | 'failed';

export interface ChatHelper {
  id: string;
  /** The helper's task title (its pane title), else its agent's name. */
  title: string;
  /** The agent's name, shown beside the title; null when it is the title already. */
  agent: string | null;
  status: ChatHelperStatus;
  /** When its pane was created (ms since the epoch): puts it in a turn. */
  startedAt: number;
}

interface PaneLike {
  info: { id: string; title?: string | null; created_at: number; parent_pane_id?: string | null };
  status: string;
}

export const HELPER_STATUS_LABEL: Record<ChatHelperStatus, string> = {
  working: 'working',
  'needs-you': 'needs you',
  done: 'done',
  failed: 'failed',
};

/** `asking`: the pane has a card waiting on the person (a question, an approval), whatever its status says. */
export function helperStatus(paneStatus: string, asking: boolean): ChatHelperStatus {
  if (paneStatus === 'error') return 'failed';
  if (asking || paneStatus === 'waiting_input') return 'needs-you';
  if (paneStatus === 'running') return 'working';
  return 'done';
}

/** The panes whose live parent is `parentId`, oldest first. */
export function chatHelpers<T extends PaneLike>(
  list: ReadonlyArray<T>,
  parentId: string,
  agentNameOf: (pane: T) => string,
  asking?: ReadonlySet<string>,
): ChatHelper[] {
  return list
    .filter((p) => liveParentId(list, p.info.id) === parentId)
    .map((p) => {
      const title = p.info.title?.trim();
      const agent = agentNameOf(p);
      return {
        id: p.info.id,
        title: title || agent,
        agent: title ? agent : null,
        status: helperStatus(p.status, asking?.has(p.info.id) ?? false),
        startedAt: p.info.created_at,
      };
    })
    .sort((a, b) => a.startedAt - b.startedAt || a.id.localeCompare(b.id));
}

/** The first parseable time of a turn: its message, else its first step. */
function turnStart(turn: ChatTurn): number | null {
  const stamps = [turn.user?.at, ...turn.steps.map((s) => (s.kind === 'tool' ? s.call.at : s.item.at))];
  for (const at of stamps) {
    const ms = at ? Date.parse(at) : NaN;
    if (Number.isFinite(ms)) return ms;
  }
  return null;
}

/** A pane's clock and the session file's are the same PC's, but a line is stamped when written: allow a little. */
const SLACK_MS = 1500;

/**
 * Which turn each helper belongs to: the last turn that had started when the helper's pane was created. A helper
 * older than every known turn goes into the first; when no turn has a time, every helper goes into the last turn
 * (the one in progress). Keys are turn ids. With no turns there is nothing to place a helper in.
 */
export function placeHelpers(turns: ReadonlyArray<ChatTurn>, helpers: ReadonlyArray<ChatHelper>): Map<string, ChatHelper[]> {
  const placed = new Map<string, ChatHelper[]>();
  if (turns.length === 0) return placed;
  const starts = turns.map(turnStart);
  const timed = starts.some((s) => s !== null);
  for (const helper of helpers) {
    let index = turns.length - 1;
    if (timed) {
      index = 0;
      starts.forEach((start, i) => {
        if (start !== null && start <= helper.startedAt + SLACK_MS) index = i;
      });
    }
    const id = turns[index].id;
    placed.set(id, [...(placed.get(id) ?? []), helper]);
  }
  return placed;
}
