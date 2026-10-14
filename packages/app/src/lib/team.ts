// Team ("Heads-up"): what the cockpit shows about teammates' pushes and my own.
//
// The Rust side (src-tauri/src/team) holds everything; this file is the typed command surface and
// the copy. A teammate's event is untrusted data: the cockpit renders it as plain text only (never
// as HTML or Markdown), the note is always shown in quotes, and nothing here runs on arrival.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { WorktreeDiff } from './cockpit-api';

export type TeamEventKind =
  | 'pushed'
  | 'pr_opened'
  | 'pr_ready'
  | 'pr_updated'
  | 'pr_merged'
  | 'pr_closed'
  | 'review_requested'
  | string;

export interface TeamEvent {
  v: number;
  id: string;
  team: string;
  project: { key: string; name: string };
  actor: { member: string; device: string; display: string; agent?: { label: string; role?: string } };
  kind: TeamEventKind;
  atMs: number;
  subject: {
    branch?: string;
    base?: string;
    commit?: string;
    forced?: boolean;
    pr?: { number: number; title: string; url: string; state: string; draft: boolean; files?: number; additions?: number; deletions?: number };
    paths?: string[];
    stats?: { files: number; additions: number; deletions: number };
  };
  note?: string;
  to?: string[];
  tierHint: 'interrupt' | 'ambient' | 'digest';
  dedupeKey: string;
  expiresAtMs: number;
  source: string;
}

/** "Tell the team?" for one of my pushes. Never sent without a click. */
export interface TeamDraft {
  id: string;
  event: TeamEvent;
  repo: string;
  createdAtMs: number;
}

export interface LocalFacts {
  projectPath: string;
  currentBranch: string | null;
  dirty: boolean;
  upToDate: boolean;
  canPull: boolean;
  canQueue: boolean;
  note: string | null;
}

export interface PaneChoice {
  paneId: string;
  label: string;
  status: string;
  title?: string | null;
}

export interface TeamCard {
  id: string;
  event: TeamEvent;
  receivedAtMs: number;
  state: 'new' | 'pulled' | 'queued' | 'dismissed';
  /** My copy of the project; null when it isn't on this computer. */
  local: LocalFacts | null;
  /** Agent panes open in that project (for Queue my agent). */
  panes: PaneChoice[];
}

export type JobState = 'waiting' | 'working' | 'ready' | 'failed' | 'applied' | 'discarded';

/** A "Queue my agent" update and its result. */
export interface TeamJob {
  id: string;
  cardId: string;
  project: string;
  paneId?: string | null;
  agentType: string;
  agentLabel: string;
  who: string;
  theirBranch: string;
  commit: string;
  note?: string | null;
  into: string;
  state: JobState;
  handle: string;
  copyBranch?: string | null;
  conflicts: string[];
  agentRan: boolean;
  stats?: { files: number; additions: number; deletions: number } | null;
  message?: string | null;
  when: string;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface TeamState {
  enabled: boolean;
  active: boolean;
  me: string;
  members: string[];
  transport: string | null;
  error: string | null;
  drafts: TeamDraft[];
  cards: TeamCard[];
  jobs: TeamJob[];
}

export interface TeamConfigView {
  enabled: boolean;
  team: string;
  folder: string;
  name: string;
  members: string[];
  projects: string[];
  profile: string | null;
}

export interface TeamConfigInput {
  enabled: boolean;
  team?: string;
  folder: string;
  name: string;
  members: string[];
  projects: string[];
}

export interface TeamApi {
  state: () => Promise<TeamState>;
  configGet: () => Promise<TeamConfigView>;
  configSet: (config: TeamConfigInput) => Promise<TeamConfigView>;
  send: (draftId: string, note?: string) => Promise<void>;
  dismissDraft: (draftId: string) => Promise<void>;
  dismissCard: (cardId: string) => Promise<void>;
  pull: (cardId: string) => Promise<string>;
  queue: (cardId: string, paneId: string, when: 'idle' | 'now') => Promise<TeamJob>;
  apply: (jobId: string) => Promise<string>;
  discard: (jobId: string) => Promise<void>;
  diff: (jobId: string) => Promise<WorktreeDiff>;
  onChanged: (cb: () => void) => Promise<UnlistenFn>;
  onDraft: (cb: (draft: TeamDraft) => void) => Promise<UnlistenFn>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noop: UnlistenFn = () => {};

function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error(`Desktop shell required: ${command}`));
  return invoke<T>(command, args);
}

export const teamApi: TeamApi = {
  state: () => call('team_state'),
  configGet: () => call('team_config_get'),
  configSet: (config) => call('team_config_set', { config }),
  send: (draftId, note) => call('team_send', { draftId, note: note ?? null }),
  dismissDraft: (draftId) => call('team_draft_dismiss', { draftId }),
  dismissCard: (cardId) => call('team_card_dismiss', { cardId }),
  pull: (cardId) => call('team_pull', { cardId }),
  queue: (cardId, paneId, when) => call('team_queue', { cardId, paneId, when }),
  apply: (jobId) => call('team_job_apply', { jobId }),
  discard: (jobId) => call('team_job_discard', { jobId }),
  diff: (jobId) => call('team_job_diff', { jobId }),
  onChanged: (cb) => (isTauri() ? listen('team://changed', () => cb()) : Promise.resolve(noop)),
  onDraft: (cb) => (isTauri() ? listen<TeamDraft>('team://draft', (e) => cb(e.payload)) : Promise.resolve(noop)),
};

// ------------------------------------------------------------------------------------- copy

/** Longest note a teammate sees (the Rust side cuts there too). */
export const MAX_NOTE = 280;

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

/** "Maya's Claude Code" or "Maya". */
export function whoDid(event: TeamEvent): string {
  const agent = event.actor.agent?.label;
  return agent ? `${event.actor.display}'s ${agent}` : event.actor.display;
}

/** The bold first line of a teammate's card. */
export function cardTitle(event: TeamEvent): string {
  const who = whoDid(event);
  const pr = event.subject.pr;
  if (pr && (event.kind === 'pr_opened' || event.kind === 'pr_ready')) return `${who} opened #${pr.number}`;
  if (pr && event.kind === 'pr_updated') return `${who} updated #${pr.number}`;
  const branch = event.subject.branch ?? 'a branch';
  return event.subject.forced ? `${who} force-pushed ${branch}` : `${who} pushed ${branch}`;
}

/** "3 files +10 −2", or "" when unknown. */
export function statsText(stats?: { files: number; additions: number; deletions: number } | null): string {
  if (!stats || stats.files === 0) return '';
  return `${plural(stats.files, 'file')} +${stats.additions} −${stats.deletions}`;
}

/** The muted second line: project · branch · files ± · PR title. */
export function cardMeta(event: TeamEvent): string {
  const parts = [event.project.name];
  const pr = event.subject.pr;
  if (pr?.title) parts.push(pr.title);
  if (event.subject.branch) parts.push(event.subject.base ? `${event.subject.branch} → ${event.subject.base}` : event.subject.branch);
  const stats = statsText(event.subject.stats);
  if (stats) parts.push(stats);
  return parts.filter(Boolean).join(' · ');
}

/** Merge when green applies to a GitHub pull request only. */
export function isGithubPr(event: TeamEvent): boolean {
  return !!event.subject.pr && event.project.key.startsWith('github.com/');
}

/** "Tell Leo", "Tell Leo and Sam", "Tell Leo, Sam and Priya", "Tell the team". */
export function tellLabel(members: string[]): string {
  if (members.length === 0) return 'Tell the team';
  if (members.length === 1) return `Tell ${members[0]}`;
  if (members.length <= 3) return `Tell ${members.slice(0, -1).join(', ')} and ${members[members.length - 1]}`;
  return 'Tell the team';
}

/** Headline of the "Tell the team?" prompt for my own push. */
export function draftTitle(draft: TeamDraft): string {
  const e = draft.event;
  const pr = e.subject.pr;
  if (pr && e.kind === 'pr_opened') return `PR #${pr.number} is open${pr.title ? ` · ${pr.title}` : ''}`;
  const who = e.actor.agent?.label ?? 'You';
  const stats = statsText(e.subject.stats);
  return `${who} pushed ${e.subject.branch ?? 'a branch'}${stats ? ` · ${stats}` : ''}`;
}

/** Which actions a teammate card shows. At most two buttons, plus Dismiss. */
export interface CardActions {
  primary: 'pull' | 'queue' | null;
  /** "Merge when green", shown disabled in v0 (GitHub pull requests only). */
  mergeWhenGreen: boolean;
  /** Why nothing is offered (a plain sentence), when it isn't. */
  note: string | null;
}

export function cardActions(card: TeamCard): CardActions {
  const mergeWhenGreen = isGithubPr(card.event);
  if (!card.local) return { primary: null, mergeWhenGreen, note: "This project isn't open here. Open an agent in it to pull or queue." };
  if (card.state === 'queued') return { primary: null, mergeWhenGreen, note: 'Queued for your agent.' };
  if (card.state === 'pulled') return { primary: null, mergeWhenGreen, note: 'Pulled.' };
  if (card.local.canPull) return { primary: 'pull', mergeWhenGreen, note: null };
  if (card.local.canQueue) return { primary: 'queue', mergeWhenGreen, note: null };
  return { primary: null, mergeWhenGreen, note: card.local.note };
}

/** The bold line of a queued update's card. */
export function jobTitle(job: TeamJob): string {
  switch (job.state) {
    case 'waiting':
      return `Queued: bring ${job.theirBranch} into ${job.into}`;
    case 'working':
      return job.agentRan ? `${job.agentLabel} is merging ${job.theirBranch}` : `Merging ${job.theirBranch}`;
    case 'ready':
      return `${job.into} is up to date, in a separate copy`;
    case 'failed':
      return `Couldn't bring ${job.theirBranch} into ${job.into}`;
    case 'applied':
      return `Applied to ${job.into}`;
    default:
      return `Discarded the update of ${job.into}`;
  }
}

/** The muted line under it. */
export function jobSub(job: TeamJob, paneName?: string): string {
  if (job.state === 'waiting') {
    return job.when === 'now' ? 'Starting now.' : `Starts when ${paneName ?? job.agentLabel} finishes its current turn.`;
  }
  if (job.state === 'working') {
    return job.agentRan ? `Resolving ${plural(job.conflicts.length, 'conflict')} in a separate copy.` : 'In a separate copy.';
  }
  const stats = statsText(job.stats);
  return [job.message, stats].filter(Boolean).join(' · ');
}

export const plain = { plural };
