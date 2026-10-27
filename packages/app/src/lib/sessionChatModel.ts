// The chat model shared by the desktop cockpit and the phone: the item types, how items merge and how they group
// into turns. Pure code with no Tauri import, so the phone bundle can use it. The desktop's command surface is in
// sessionChat.ts (which re-exports all of this); the phone reads the same items over the bridge (PhoneChatView).
// Agent text and tool output are data: the views show them as plain text only.

export type TurnOutcome = 'completed' | 'interrupted' | 'failed';

/** A task's state in the agent's list. */
export type PlanStatus = 'pending' | 'in_progress' | 'completed';

/**
 * One entry of a `plan` item as sent. In a full list (`replace: true`) every entry has a `status`. In an update
 * (`replace: false`) an entry finds its task by `taskId`: a new id adds a task, `status` may be missing (only the
 * text changed), `text` may be empty (only the status changed) and `deleted` removes the task.
 */
export interface PlanEntry {
  text: string;
  status?: PlanStatus | 'deleted';
  taskId?: string;
}

/** One task of the folded list (what a view shows). */
export interface PlanItem {
  text: string;
  status: PlanStatus;
  taskId?: string;
}

/** One parsed line of the session file. Mirrors `ChatItem` in src-tauri/src/session_chat (camelCase). */
export type ChatItem =
  | { kind: 'user'; id: string; at: string | null; text: string }
  | { kind: 'assistant'; id: string; at: string | null; text: string; model: string | null; finalAnswer: boolean }
  | {
      kind: 'toolCall';
      id: string;
      at: string | null;
      callId: string;
      name: string;
      summary: string;
      /** The tool's input JSON, strings cut and the whole near 8k characters (old/new text of an edit, a patch). */
      input?: unknown;
      /** Files the call touches, in order. */
      paths?: string[];
      /** The call changes the files in `paths` (an edit or write, not a read). */
      writes?: boolean;
      /** The call starts a helper agent (a sub-agent thread). */
      helper?: boolean;
    }
  | { kind: 'toolResult'; id: string; at: string | null; callId: string; ok: boolean; output: string }
  | { kind: 'turnEnd'; id: string; at: string | null; outcome: TurnOutcome; durationMs: number | null }
  | {
      kind: 'usage';
      id: string;
      at: string | null;
      inputTokens: number;
      outputTokens: number;
      contextWindow: number | null;
    }
  | { kind: 'session'; id: string; at: string | null; model: string | null; cliVersion: string | null }
  | { kind: 'plan'; id: string; at: string | null; items: PlanEntry[]; replace: boolean }
  /** `noticeKind`: `compacted`, `error`, `pr` or `title`; another value is shown as a plain note. */
  | { kind: 'notice'; id: string; at: string | null; noticeKind: string; text: string; url?: string | null };

export type UserItem = Extract<ChatItem, { kind: 'user' }>;
export type AssistantItem = Extract<ChatItem, { kind: 'assistant' }>;
export type ToolCallItem = Extract<ChatItem, { kind: 'toolCall' }>;
export type ToolResultItem = Extract<ChatItem, { kind: 'toolResult' }>;
export type TurnEndItem = Extract<ChatItem, { kind: 'turnEnd' }>;
export type UsageItem = Extract<ChatItem, { kind: 'usage' }>;
export type PlanChatItem = Extract<ChatItem, { kind: 'plan' }>;
export type NoticeItem = Extract<ChatItem, { kind: 'notice' }>;

/**
 * Where a pane's chat is read from: the agent's own session file, the agent's local session API (an agent that serves
 * its session; the terminal's text is read while the API does not answer), or the terminal's text (an agent with
 * neither).
 */
export type ChatSource = 'file' | 'api' | 'screen';

/** `session_chat_open` answer: whether this pane has a chat we can read, where from, and the items so far. */
export interface SessionChatSnapshot {
  available: boolean;
  reason: string | null;
  items: ChatItem[];
  /** Where the items come from; null while the chat is unavailable. Older backends send nothing (a file). */
  source?: ChatSource | null;
  /** While there are no items: one short line saying what the reader is waiting for (where it looks). */
  waitingFor?: string | null;
}

/** Payload of the `session-chat-items` event. `reset`: the file was replaced or truncated, drop old items. */
export interface SessionChatItemsEvent {
  paneId: string;
  items: ChatItem[];
  reset: boolean;
}

/** Payload of the `session-chat-status` event: what an empty chat is waiting for changed (`null`: nothing). */
export interface SessionChatStatusEvent {
  paneId: string;
  waitingFor: string | null;
}

/** Payload of the `session-chat-source` event: a pane's chat is now read from another source (the API went or came back). */
export interface SessionChatSourceEvent {
  paneId: string;
  source: ChatSource;
}

export type ToolStatus = 'running' | 'ok' | 'error';

export interface ToolStep {
  kind: 'tool';
  id: string;
  call: ToolCallItem;
  /** Null until the result arrives: the tool is still running. */
  result: ToolResultItem | null;
  status: ToolStatus;
}

export interface TextStep {
  kind: 'text';
  id: string;
  item: AssistantItem;
}

/** Something that happened to the session (compaction, an error, a pull request): shown in the turn it came in. */
export interface NoticeStep {
  kind: 'notice';
  id: string;
  item: NoticeItem;
}

export type TurnStep = TextStep | ToolStep | NoticeStep;

/** One exchange: what the person typed, then the agent's steps, then how the turn ended (if it did). */
export interface ChatTurn {
  id: string;
  /** Null for agent output before any person's message (a session opened mid-turn). */
  user: UserItem | null;
  steps: TurnStep[];
  end: TurnEndItem | null;
  outcome: TurnOutcome | null;
  durationMs: number | null;
  /** The task list as it stood after this turn's last plan update; null when the turn did not update it. */
  plan: PlanItem[] | null;
}

/** A file the agent's tool calls touched. */
export interface OutputFile {
  path: string;
  /** The name of the last tool that touched it. */
  tool: string;
  /** Some call changed it (an edit or write); false when it was only read. */
  wrote: boolean;
}

export interface GroupedChat {
  turns: ChatTurn[];
  /** Latest known model (a `session` or `assistant` item) and CLI version (a `session` item). */
  model: string | null;
  cliVersion: string | null;
  /** Latest `usage` item. */
  usage: UsageItem | null;
  /** The task list now (every plan update folded in); empty when the agent made none. */
  plan: PlanItem[];
  /** The newest session title a `title` notice gave. */
  title: string | null;
  /** Files touched by tool calls: unique paths in order of first touch. */
  outputs: OutputFile[];
}

/** Whether a pane's chat can be read at all. */
export type SessionChatState = 'loading' | 'ready' | 'unavailable';

/**
 * `GET /panes/{id}/chat?since=<rev>` (the phone): the newest items, or only what changed since revision `since`.
 * `reset`: drop the list held and show `items` instead (the file was replaced, or the phone fell too far behind).
 */
export interface PhoneChatReply {
  available: boolean;
  reason: string | null;
  source: ChatSource | null;
  waitingFor: string | null;
  /** Ask for what is newer than this next time. */
  rev: number;
  reset: boolean;
  items: ChatItem[];
}

/**
 * Add `incoming` to `prev`, one entry per `id`. An item whose `id` is already there is replaced by the newest copy
 * in place, so the order is the order of first arrival (a chat read from the screen sends a growing reply again
 * under the same id). `reset` drops `prev` first (the file was replaced, or an item went away).
 * Only the latest `usage` item is kept: every agent message carries one and only the newest is shown.
 */
export function mergeItems(prev: readonly ChatItem[], incoming: readonly ChatItem[], reset: boolean): ChatItem[] {
  const byId = new Map<string, ChatItem>();
  if (!reset) for (const item of prev) byId.set(item.id, item);
  for (const item of incoming) byId.set(item.id, item);
  const merged = [...byId.values()];
  const latestUsage = merged.map((item) => item.kind).lastIndexOf('usage');
  return merged.filter((item, index) => item.kind !== 'usage' || index === latestUsage);
}

/**
 * Fold one `plan` item into the task list `plan` and return the new list (`plan` itself is not changed).
 * A full list (`replace`) replaces everything. An update changes the task with its `taskId`: unknown ids are added
 * (an update for a task created before the list started shows as "Task <id>"), `deleted` removes, an empty `text`
 * or a missing `status` keeps what the task had. An update with no `taskId` is ignored.
 */
export function foldPlan(plan: readonly PlanItem[], item: PlanChatItem): PlanItem[] {
  const entries = Array.isArray(item.items) ? item.items : [];
  if (item.replace) {
    return entries
      .filter((entry) => entry && typeof entry.text === 'string')
      .map((entry) => ({
        text: entry.text,
        status: entry.status === 'in_progress' || entry.status === 'completed' ? entry.status : 'pending',
        ...(entry.taskId ? { taskId: entry.taskId } : {}),
      }));
  }
  let next = [...plan];
  for (const entry of entries) {
    if (!entry || !entry.taskId) continue;
    const at = next.findIndex((task) => task.taskId === entry.taskId);
    if (entry.status === 'deleted') {
      if (at >= 0) next.splice(at, 1);
      continue;
    }
    const status = entry.status ?? (at >= 0 ? next[at]!.status : 'pending');
    const text = entry.text || (at >= 0 ? next[at]!.text : `Task ${entry.taskId}`);
    const task: PlanItem = { text, status, taskId: entry.taskId };
    if (at >= 0) next[at] = task;
    else next = [...next, task];
  }
  return next;
}

/**
 * Group items into turns. A turn starts at a `user` item and ends at its `turnEnd`. A tool result attaches
 * to the call with the same `callId` (a call without one is still running). Calls that repeat a `callId`
 * (a tool shown first as started, then as finished) are one row: the latest call item replaces the first.
 * Agent output before any user item starts a turn with no `user`.
 *
 * A `turnEnd` that comes after the turn already ended (an agent writes the turn's real length in a later line) only
 * fills in the turn's `durationMs`. `plan` items are folded (see `foldPlan`) into the session `plan` and, per turn,
 * the list as it stood after the turn's last update. A `notice` becomes a step of the turn it came in (after the
 * turn ended: of the turn before), except `title`, which sets `title`, and a pull request link seen before. The
 * files of tool calls are collected into `outputs`. Items of a kind this code does not know are ignored.
 */
export function groupTurns(items: readonly ChatItem[]): GroupedChat {
  const turns: ChatTurn[] = [];
  const calls = new Map<string, ToolStep>();
  const outputs = new Map<string, OutputFile>();
  const seenLinks = new Set<string>();
  let current: ChatTurn | null = null;
  let model: string | null = null;
  let cliVersion: string | null = null;
  let usage: UsageItem | null = null;
  let plan: PlanItem[] = [];
  let title: string | null = null;

  const newTurn = (id: string, user: UserItem | null): ChatTurn => {
    const turn: ChatTurn = { id, user, steps: [], end: null, outcome: null, durationMs: null, plan: null };
    turns.push(turn);
    return turn;
  };
  const turnFor = (id: string): ChatTurn => {
    current ??= newTurn(id, null);
    return current;
  };

  for (const item of items) {
    if (!item || typeof item !== 'object') continue;
    switch (item.kind) {
      case 'user':
        current = newTurn(item.id, item);
        break;
      case 'assistant':
        turnFor(item.id).steps.push({ kind: 'text', id: item.id, item });
        // A session item need not name the model (it is on each message); the newest one that does wins.
        model = item.model ?? model;
        break;
      case 'toolCall': {
        for (const path of Array.isArray(item.paths) ? item.paths : []) {
          const known = outputs.get(path);
          outputs.set(path, { path, tool: item.name, wrote: item.writes === true || known?.wrote === true });
        }
        const known = calls.get(item.callId);
        if (known) {
          known.call = item;
          break;
        }
        const step: ToolStep = { kind: 'tool', id: item.id, call: item, result: null, status: 'running' };
        calls.set(item.callId, step);
        turnFor(item.id).steps.push(step);
        break;
      }
      case 'toolResult': {
        const step = calls.get(item.callId);
        if (step) {
          step.result = item;
          step.status = item.ok ? 'ok' : 'error';
        }
        break;
      }
      case 'turnEnd': {
        const last = turns[turns.length - 1];
        if (!current && last?.end) {
          // A late line for a turn that already ended: only the length is new.
          last.durationMs = item.durationMs ?? last.durationMs;
          break;
        }
        const turn = turnFor(item.id);
        turn.end = item;
        turn.outcome = item.outcome;
        turn.durationMs = item.durationMs;
        current = null;
        break;
      }
      case 'usage':
        usage = item;
        break;
      case 'session':
        model = item.model ?? model;
        cliVersion = item.cliVersion ?? cliVersion;
        break;
      case 'plan':
        plan = foldPlan(plan, item);
        (current ?? turns[turns.length - 1] ?? turnFor(item.id)).plan = plan;
        break;
      case 'notice': {
        if (item.noticeKind === 'title') {
          title = item.text || title;
          break;
        }
        if (item.url) {
          if (seenLinks.has(item.url)) break;
          seenLinks.add(item.url);
        }
        (current ?? turns[turns.length - 1] ?? turnFor(item.id)).steps.push({ kind: 'notice', id: item.id, item });
        break;
      }
    }
  }
  return { turns, model, cliVersion, usage, plan, title, outputs: [...outputs.values()] };
}
