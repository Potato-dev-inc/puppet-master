// Companion v1 wire types and transports (SPEC-1028 sections 2.2 and 11). The names mirror the Rust serde shapes in
// src-tauri/src/companion/model.rs in camelCase. Desktop talks to Tauri commands, the phone to the bridge routes;
// the same split as paneOutbox.ts / paneOutbox-bridge.ts. Nothing here draws: the UI reads these values.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { BridgeClient } from './bridge-types';
import { bridgeRefusalText } from './paneOutbox-bridge';

/** The eight faces (components/companion/avatars.ts). */
export type AvatarId = 'mochi' | 'bit' | 'nib' | 'tofu' | 'lumen' | 'hoot' | 'sprout' | 'kit';

/** Preset ids from the setup sheet; `custom` means the person wrote the SOUL.md text. */
export type CompanionPersona = 'cheerful' | 'calm' | 'coach' | 'plain' | 'custom';

/** Local time, 0..=23 and 0..=59. */
export interface HhMm {
  h: number;
  m: number;
}

export type When =
  | { kind: 'daily'; at: HhMm }
  | { kind: 'weekdays'; at: HhMm }
  | { kind: 'weekly'; day: number; at: HhMm }
  | { kind: 'every'; minutes: number }
  | { kind: 'once'; atMs: number };

export interface CheckIn {
  id: string;
  label: string;
  text: string;
  when: When;
  enabled: boolean;
}

export type CompanionState = 'starting' | 'idle' | 'working' | 'needs_you' | 'sleeping' | 'stopped';

export interface NeedsRef {
  itemId: string;
  title: string;
}

export interface NextCheckIn {
  id: string;
  label: string;
  atMs: number;
}

/** What the desktop and the phone read. The phone copy has no `folder`. */
export interface CompanionView {
  id: string;
  name: string;
  avatar: AvatarId;
  persona: CompanionPersona;
  agentType: string;
  agentLabel: string;
  folder?: string;
  folderName: string;
  state: CompanionState;
  stateLine: string;
  paneId: string | null;
  needs: NeedsRef | null;
  pendingMemory: number;
  nextCheckin: NextCheckIn | null;
  /** Desktop only: the full check-in list for the editor. The phone copy has none. */
  checkins?: CheckIn[];
  restarts: number;
  autoStart: boolean;
  /** Whether a reply pushes the phones that may watch. Missing on a view from before the setting: on. */
  pushReplies?: boolean;
  /** Whether the companion sees what the person's other agents did today. Missing on an older view: off. */
  shareToday?: boolean;
  /** The id of the agent it moved from. Missing or null: it was never moved. */
  previousAgent?: string | null;
  /** That agent's display name. */
  previousAgentLabel?: string | null;
  /** When a timed move ends and it goes back (epoch ms). Null: it stays. */
  untilMs?: number | null;
  /** A move that waits until the companion is free ("Moves when Mochi is free"). */
  queuedMove?: QueuedMove | null;
  /**
   * Desktop only. The home folder is too broad (the Desktop, a drive, a folder of projects), so other agents below it
   * read the companion's brief. Missing or null: the home is fine.
   */
  homeProblem?: HomeProblem | null;
}

/** Why a companion's home is a problem, and where `Move to its own folder` would put it. */
export interface HomeProblem {
  /** In a few words: "your Desktop", "a folder with other projects in it". */
  place: string;
  suggestedFolder: string;
}

export interface QueuedMove {
  agentType: string;
  agentLabel: string;
  untilMs: number | null;
}

export type PendingKind = 'note' | 'user' | 'forget' | 'skill';
export type PendingDecision = 'keep' | 'drop';
export type MemoryFile = 'soul' | 'user' | 'memory';

export interface PendingItem {
  id: string;
  kind: PendingKind;
  text: string;
  flagged: string | null;
  targetLine: string | null;
  /** A skill's steps. Only on the computer: the phone sees the title (`text`) and nothing more. */
  body?: string;
}

/** The limits of a skill (`companion/model.rs`): characters in the whole file, and how many a companion may have. */
export const SKILL_CAP = 4096;
export const MAX_SKILLS = 30;

/** One skill in a list: no steps, so the phone can read it too. */
export interface SkillRef {
  slug: string;
  title: string;
  chars: number;
  /** The file changed outside Puppet Master (or was never approved here). The brief does not list it until it is saved. */
  unreviewed: boolean;
  flagged: string | null;
}

/** A skill opened for reading or editing (desktop only). */
export interface SkillFull extends SkillRef {
  body: string;
}

export type FeedKind = 'reply' | 'checkin' | 'missed' | 'woke' | 'slept' | 'napped' | 'restarted' | 'needs_you' | 'memory' | 'moved' | 'delegated';

export interface FeedItem {
  id: string;
  atMs: number;
  kind: FeedKind;
  text: string;
}

export interface MemoryView {
  soul: string;
  user: string;
  memory: string;
  caps: { soul: number; user: number; memory: number };
  pending: PendingItem[];
  /** Desktop only: a file changed outside Puppet Master. The phone gets []. */
  outsideChanges: { file: MemoryFile }[];
  /** The skills it has learned, titles only. Missing on a PC from before skills: none. */
  skills?: SkillRef[];
}

/** One agent the setup sheet can pick (`companion_agents`). `reason` is the plain sentence when `ok` is false. */
export interface CompanionAgentOption {
  id: string;
  label: string;
  ok: boolean;
  reason: string | null;
}

/** What the setup sheet sends to `companion_create`. Optional fields take the backend defaults. */
export interface CompanionDraft {
  name: string;
  avatar: AvatarId;
  persona: CompanionPersona;
  agentType: string;
  launchArgs?: string[];
  account?: string | null;
  folder: string;
  autoStart?: boolean;
  memoryReview?: boolean;
  pushReplies?: boolean;
  shareToday?: boolean;
  napAt?: HhMm | null;
  checkins?: CheckIn[];
}

/**
 * What `companion_update` changes. A new `agentType` is a move: it runs at once when the companion is free and
 * waits ("Moves when Mochi is free") when it is working or has a card open. `untilMs` (with a move): when it
 * goes back to the agent it left; null or missing means it stays.
 */
export type CompanionPatch = Partial<CompanionDraft> & { untilMs?: number | null };

export interface CompanionTransport {
  list(): Promise<CompanionView[]>;
  get(id: string): Promise<CompanionView>;
  wake(id: string): Promise<CompanionView>;
  sleep(id: string): Promise<CompanionView>;
  memory(id: string): Promise<MemoryView>;
  decidePending(id: string, itemId: string, decision: PendingDecision): Promise<MemoryView>;
  feed(id: string, sinceMs?: number): Promise<FeedItem[]>;
  // Desktop only. The bridge transport refuses these with a plain sentence.
  agents(): Promise<CompanionAgentOption[]>;
  create(input: CompanionDraft): Promise<CompanionView>;
  update(id: string, patch: CompanionPatch): Promise<CompanionView>;
  archive(id: string): Promise<void>;
  restart(id: string): Promise<CompanionView>;
  /** Moves the companion out of a broad home into a folder of its own. An error leaves everything as it was. */
  rehome(id: string): Promise<CompanionView>;
  saveMemory(id: string, file: MemoryFile, text: string): Promise<MemoryView>;
  decideOutsideChange(id: string, file: MemoryFile, decision: 'keep' | 'undo'): Promise<MemoryView>;
  tidyUp(id: string): Promise<void>;
  /** Approve a proposed skill. `edit` carries the person's own title and steps; without it, as proposed. */
  approveSkill(id: string, itemId: string, edit?: { title: string; body: string }): Promise<MemoryView>;
  getSkill(id: string, slug: string): Promise<SkillFull>;
  saveSkill(id: string, slug: string, title: string, body: string): Promise<MemoryView>;
  /** Moves the file into `skills/.removed/` in the home folder. */
  removeSkill(id: string, slug: string): Promise<MemoryView>;
  setCheckIns(id: string, checkins: CheckIn[]): Promise<CompanionView>;
  openFolder(id: string): Promise<void>;
  /** Desktop: the `companion://changed` event with every companion. */
  onChanged?(cb: (companions: CompanionView[]) => void): Promise<UnlistenFn>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const DESKTOP_ONLY = 'Companions are set up on the computer.';

async function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw new Error(typeof error === 'string' ? error : error instanceof Error ? error.message : 'Something went wrong.');
  }
}

/** The desktop transport: the `companion_*` Tauri commands (SPEC section 11). */
export const tauriCompanionTransport: CompanionTransport = {
  list: () => call<CompanionView[]>('companion_list', {}),
  get: (id) => call<CompanionView>('companion_get', { id }),
  wake: (id) => call<CompanionView>('companion_wake', { id }),
  sleep: (id) => call<CompanionView>('companion_sleep', { id }),
  memory: (id) => call<MemoryView>('companion_memory_get', { id }),
  decidePending: (id, itemId, decision) => call<MemoryView>('companion_pending_decide', { id, itemId, decision }),
  feed: (id, sinceMs = 0) => call<FeedItem[]>('companion_feed', { id, sinceMs }),
  agents: () => call<CompanionAgentOption[]>('companion_agents', {}),
  create: (input) => call<CompanionView>('companion_create', { input }),
  update: (id, patch) => call<CompanionView>('companion_update', { id, patch }),
  archive: (id) => call<void>('companion_archive', { id }),
  restart: (id) => call<CompanionView>('companion_restart', { id }),
  rehome: (id) => call<CompanionView>('companion_rehome', { id }),
  saveMemory: (id, file, text) => call<MemoryView>('companion_memory_save', { id, file, text }),
  decideOutsideChange: (id, file, decision) => call<MemoryView>('companion_outside_change_decide', { id, file, decision }),
  tidyUp: (id) => call<void>('companion_tidy_up', { id }),
  approveSkill: (id, itemId, edit) =>
    call<MemoryView>('companion_skill_approve', { id, itemId, title: edit?.title ?? null, body: edit?.body ?? null }),
  getSkill: (id, slug) => call<SkillFull>('companion_skill_get', { id, slug }),
  saveSkill: (id, slug, title, body) => call<MemoryView>('companion_skill_save', { id, slug, title, body }),
  removeSkill: (id, slug) => call<MemoryView>('companion_skill_remove', { id, slug }),
  setCheckIns: (id, checkins) => call<CompanionView>('companion_checkins_set', { id, checkins }),
  openFolder: (id) => call<void>('companion_open_folder', { id }),
  onChanged: (cb) => {
    if (!isTauri()) return Promise.resolve((() => {}) as UnlistenFn);
    return listen<CompanionView[]>('companion://changed', (e) => cb(Array.isArray(e.payload) ? e.payload : []));
  },
};

/**
 * The phone transport: the `/companions` bridge routes (SPEC section 11). The bridge client has one method per
 * route; an old PC without them gets a plain sentence. Desktop-only calls are refused.
 */
export function bridgeCompanionTransport(bridge: BridgeClient): CompanionTransport {
  type Route<K extends keyof BridgeClient> = NonNullable<BridgeClient[K]>;
  // Runs one bridge route. An old PC without the method is told so; a refused call reads as the PC's plain sentence,
  // the same way the message box reads its errors.
  const route = <K extends keyof BridgeClient, T>(name: K, run: (fn: Route<K>) => Promise<T>): Promise<T> => {
    const fn: unknown = bridge[name];
    if (typeof fn !== 'function') return Promise.reject(new Error('This PC is too old to show companions on the phone.'));
    return run(fn.bind(bridge) as Route<K>).catch((error: unknown) => {
      throw new Error(bridgeRefusalText(error, 'Could not reach your companion.'));
    });
  };
  const refuse = async (): Promise<never> => {
    throw new Error(DESKTOP_ONLY);
  };

  return {
    list: () => route('companionList', (fn) => fn()),
    get: (id) => route('companionGet', (fn) => fn(id)),
    wake: (id) => route('companionWake', (fn) => fn(id)),
    sleep: (id) => route('companionSleep', (fn) => fn(id)),
    memory: (id) => route('companionMemory', (fn) => fn(id)),
    decidePending: (id, itemId, decision) => route('companionDecidePending', (fn) => fn(id, itemId, decision)),
    feed: (id, sinceMs = 0) => route('companionFeed', (fn) => fn(id, sinceMs)),
    agents: refuse,
    create: refuse,
    update: refuse,
    archive: refuse,
    restart: refuse,
    rehome: refuse,
    saveMemory: refuse,
    decideOutsideChange: refuse,
    tidyUp: refuse,
    approveSkill: refuse,
    getSkill: refuse,
    saveSkill: refuse,
    removeSkill: refuse,
    setCheckIns: refuse,
    openFolder: refuse,
  };
}

const STATE_WORD: Record<CompanionState, string> = {
  starting: 'Waking up',
  idle: 'Here',
  working: 'Working',
  needs_you: 'Needs you',
  sleeping: 'Asleep',
  stopped: 'Stopped',
};

/** The short state word for the sidebar and the face's label (SPEC section 8). */
export function stateWord(state: CompanionState): string {
  return STATE_WORD[state];
}

const DAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
const pad2 = (n: number): string => String(n).padStart(2, '0');
const clock = (at: HhMm): string => `${at.h}:${pad2(at.m)}`;

/** A check-in's schedule in plain words, e.g. "Every day at 8:00". */
export function formatWhen(when: When): string {
  switch (when.kind) {
    case 'daily':
      return `Every day at ${clock(when.at)}`;
    case 'weekdays':
      return `Weekdays at ${clock(when.at)}`;
    case 'weekly':
      return `Every ${DAYS[when.day] ?? DAYS[0]} at ${clock(when.at)}`;
    case 'every': {
      if (when.minutes % 60 === 0) {
        const hours = when.minutes / 60;
        return hours === 1 ? 'Every hour' : `Every ${hours} hours`;
      }
      return `Every ${when.minutes} minutes`;
    }
    case 'once': {
      const at = new Date(when.atMs);
      const day = at.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
      return `Once on ${day} at ${at.getHours()}:${pad2(at.getMinutes())}`;
    }
  }
}

export interface FakeCompanionSeed {
  companions?: CompanionView[];
  memory?: Partial<MemoryView>;
  feed?: FeedItem[];
  agents?: CompanionAgentOption[];
  /** The steps of the skills in `memory.skills`, by slug. */
  skillBodies?: Record<string, string>;
}

export interface FakeCompanionTransport extends CompanionTransport {
  /** Every call so far, in order: the method name and its arguments. */
  readonly calls: { method: keyof CompanionTransport; args: unknown[] }[];
}

/**
 * An in-memory transport for tests and for the screens that land before the Rust side (SPEC P3, P5). It keeps
 * companions, memory and feed in plain objects, and records each call in `calls`.
 */
export function fakeCompanionTransport(seed: FakeCompanionSeed = {}): FakeCompanionTransport {
  const companions = new Map<string, CompanionView>((seed.companions ?? []).map((c) => [c.id, { ...c }]));
  const feed = [...(seed.feed ?? [])];
  const agents = seed.agents ?? [{ id: 'claude', label: 'Claude Code', ok: true, reason: null }];
  let memory: MemoryView = {
    soul: '',
    user: '',
    memory: '',
    caps: { soul: 1500, user: 1400, memory: 2200 },
    pending: [],
    outsideChanges: [],
    ...seed.memory,
  };
  const calls: FakeCompanionTransport['calls'] = [];
  // The steps of each skill by slug (the list carries none). Seed skills start with the text in `skillBodies`.
  const skillBodies = new Map<string, string>(Object.entries(seed.skillBodies ?? {}));
  const slugOf = (title: string): string =>
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '')
      .slice(0, 24) || 'skill';
  // Records the call, then runs it. A throw inside becomes a rejected promise, like the real transports.
  const log = <T>(method: keyof CompanionTransport, args: unknown[], run: () => T): Promise<T> => {
    calls.push({ method, args });
    try {
      return Promise.resolve(run());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const find = (id: string): CompanionView => {
    const found = companions.get(id);
    if (!found) throw new Error('This companion is not here any more.');
    return found;
  };
  const put = (view: CompanionView): CompanionView => {
    companions.set(view.id, view);
    return view;
  };
  const withState = (id: string, state: CompanionState): CompanionView => put({ ...find(id), state });

  return {
    calls,
    list: () => log('list', [], () => [...companions.values()].map((c) => ({ ...c }))),
    get: (id) => log('get', [id], () => ({ ...find(id) })),
    wake: (id) => log('wake', [id], () => withState(id, 'idle')),
    sleep: (id) => log('sleep', [id], () => withState(id, 'sleeping')),
    memory: (id) => log('memory', [id], () => ({ ...memory })),
    decidePending: (id, itemId, decision) =>
      log('decidePending', [id, itemId, decision], () => {
        memory = { ...memory, pending: memory.pending.filter((p) => p.id !== itemId) };
        return { ...memory };
      }),
    feed: (id, sinceMs = 0) => log('feed', [id, sinceMs], () => feed.filter((f) => f.atMs > sinceMs).map((f) => ({ ...f }))),
    agents: () => log('agents', [], () => agents.map((a) => ({ ...a }))),
    create: (input) =>
      log('create', [input], () => {
        const view: CompanionView = {
          id: input.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'companion',
          name: input.name,
          avatar: input.avatar,
          persona: input.persona,
          agentType: input.agentType,
          agentLabel: input.agentType,
          folder: input.folder,
          folderName: input.folder.split(/[\\/]/).filter(Boolean).pop() ?? '',
          state: 'starting',
          stateLine: 'Waking up…',
          paneId: null,
          needs: null,
          pendingMemory: 0,
          nextCheckin: null,
          restarts: 0,
          autoStart: input.autoStart ?? true,
        };
        return put(view);
      }),
    update: (id, patch) =>
      log('update', [id, patch], () => {
        const next = { ...find(id) };
        if (patch.name !== undefined) next.name = patch.name;
        if (patch.avatar !== undefined) next.avatar = patch.avatar;
        if (patch.persona !== undefined) next.persona = patch.persona;
        if (patch.pushReplies !== undefined) next.pushReplies = patch.pushReplies;
        if (patch.shareToday !== undefined) next.shareToday = patch.shareToday;
        if (patch.agentType !== undefined && patch.agentType !== next.agentType) {
          // The fake moves at once. (The real one waits while the companion is busy: see Rust `moving.rs`.)
          const timedStay = next.previousAgent != null && next.untilMs != null;
          if (patch.agentType === next.previousAgent) {
            next.previousAgent = null;
            next.previousAgentLabel = null;
            next.untilMs = null;
          } else {
            if (!timedStay) {
              next.previousAgent = next.agentType;
              next.previousAgentLabel = next.agentLabel;
            }
            next.untilMs = patch.untilMs ?? null;
          }
          next.agentType = patch.agentType;
          next.agentLabel = agents.find((a) => a.id === patch.agentType)?.label ?? patch.agentType;
          next.queuedMove = null;
        } else if (patch.agentType !== undefined) {
          next.queuedMove = null;
        }
        if (patch.folder !== undefined) next.folder = patch.folder;
        if (patch.autoStart !== undefined) next.autoStart = patch.autoStart;
        return put(next);
      }),
    archive: (id) =>
      log('archive', [id], () => {
        find(id);
        companions.delete(id);
      }),
    restart: (id) => log('restart', [id], () => withState(id, 'starting')),
    rehome: (id) =>
      log('rehome', [id], () => {
        const view = find(id);
        const next = { ...view, homeProblem: null };
        if (view.homeProblem) {
          next.folder = view.homeProblem.suggestedFolder;
          next.folderName = view.homeProblem.suggestedFolder.split(/[\\/]/).filter(Boolean).pop() ?? '';
        }
        return put(next);
      }),
    saveMemory: (id, file, text) =>
      log('saveMemory', [id, file, text], () => {
        memory = { ...memory, [file]: text };
        return { ...memory };
      }),
    decideOutsideChange: (id, file, decision) =>
      log('decideOutsideChange', [id, file, decision], () => {
        memory = { ...memory, outsideChanges: memory.outsideChanges.filter((o) => o.file !== file) };
        return { ...memory };
      }),
    tidyUp: (id) => log('tidyUp', [id], () => undefined),
    approveSkill: (id, itemId, edit) =>
      log('approveSkill', [id, itemId, edit], () => {
        const item = memory.pending.find((p) => p.id === itemId && p.kind === 'skill');
        if (!item) throw new Error('That item is gone. Look at the list again.');
        const title = edit?.title ?? item.text;
        const body = edit?.body ?? item.body ?? '';
        skillBodies.set(slugOf(title), body);
        memory = {
          ...memory,
          pending: memory.pending.filter((p) => p.id !== itemId),
          skills: [
            ...(memory.skills ?? []),
            { slug: slugOf(title), title, chars: Array.from(body).length, unreviewed: false, flagged: null },
          ],
        };
        return { ...memory };
      }),
    getSkill: (id, slug) =>
      log('getSkill', [id, slug], () => {
        const ref = (memory.skills ?? []).find((s) => s.slug === slug);
        if (!ref) throw new Error('No skill has that name.');
        return { ...ref, body: skillBodies.get(slug) ?? '' };
      }),
    saveSkill: (id, slug, title, body) =>
      log('saveSkill', [id, slug, title, body], () => {
        skillBodies.set(slug, body);
        memory = {
          ...memory,
          skills: (memory.skills ?? []).map((s) =>
            s.slug === slug ? { ...s, title, chars: Array.from(body).length, unreviewed: false } : s,
          ),
        };
        return { ...memory };
      }),
    removeSkill: (id, slug) =>
      log('removeSkill', [id, slug], () => {
        memory = { ...memory, skills: (memory.skills ?? []).filter((s) => s.slug !== slug) };
        return { ...memory };
      }),
    setCheckIns: (id, checkins) => log('setCheckIns', [id, checkins], () => put({ ...find(id), checkins, nextCheckin: null })),
    openFolder: (id) => log('openFolder', [id], () => undefined),
  };
}
