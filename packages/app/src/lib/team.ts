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
  | 'ci_failed'
  | 'ci_passed'
  | 'review_requested'
  | 'please_review'
  | 'note'
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
    /** GitHub checks on a PR or a branch (ci_failed / ci_passed). */
    ci?: { conclusion: 'failure' | 'success' | 'pending' | 'none'; failedJobs: string[]; url: string };
  };
  note?: string;
  to?: string[];
  tierHint: 'interrupt' | 'ambient' | 'digest';
  dedupeKey: string;
  expiresAtMs: number;
  source: string;
}

/** "Tell the team?" for one of my pushes, or one my agent drafted. Never sent without a click. */
export interface TeamDraft {
  id: string;
  event: TeamEvent;
  repo: string;
  createdAtMs: number;
  /** Set when an agent drafted it (the MCP tool `team_notify`): the label it gave, or "An agent". */
  draftedBy?: string | null;
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

/** `needs_start`: the merge stopped on conflicts and the agent would run commands without asking, so it waits for my click. */
export type JobState = 'waiting' | 'needs_start' | 'working' | 'ready' | 'failed' | 'applied' | 'discarded';

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

export type TeamTransport = 'folder' | 'relay';
/** How the relay connection is doing (null for a shared folder or when Team is off). */
export type TeamLink = 'live' | 'polling' | 'offline';

export interface TeamState {
  enabled: boolean;
  active: boolean;
  me: string;
  /** Teammates' names (not mine). */
  members: string[];
  transport: TeamTransport | null;
  link: TeamLink | null;
  error: string | null;
  /** How many teammate updates are known to be missing. */
  missing: number;
  drafts: TeamDraft[];
  cards: TeamCard[];
  jobs: TeamJob[];
}

export type TeamRole = 'watch' | 'approve' | 'admin';

export interface MemberView {
  id: string;
  name: string;
  roles: TeamRole[];
  admin: boolean;
  me: boolean;
  github: string | null;
  githubConfirmed: boolean;
  devices: number;
}

/** Someone waiting for my yes (admin only). */
export interface JoinView {
  id: string;
  name: string;
  github: string | null;
  device: string;
  receivedAtMs: number;
  /** More than one person used this invite's code. */
  warning?: string | null;
}

export interface TeamConfigView {
  enabled: boolean;
  /** The team id. Never shown. '' means "not on a team". */
  team: string;
  teamName: string;
  relay: string;
  folder: string;
  /** My name. */
  name: string;
  projects: string[];
  profile: string | null;
  /** team.json exists but couldn't be read. */
  readError: string | null;
  error: string | null;
  /** The team's name while waiting for the admin's yes. */
  joining: string | null;
  admin: boolean;
  members: MemberView[];
  joins: JoinView[];
  epoch: number;
  /** Where the keys live, e.g. "Windows Credential Manager". */
  keyStore: string;
  transport: TeamTransport | null;
  link: TeamLink | null;
}

export interface TeamConfigInput {
  enabled: boolean;
  projects: string[];
}

/** Exactly one of `relay` / `folder` is non-empty. */
export interface TeamCreateInput {
  teamName: string;
  name: string;
  relay: string;
  folder: string;
}

export interface TeamJoinInput {
  link: string;
  code: string;
  name: string;
  github?: string | null;
}

export interface InviteView {
  link: string;
  code: string;
  expiresAtMs: number;
}

/** The "Merge when green" sheet: every field but the method is GitHub's own answer, not the card's. */
export interface MergePlan {
  /** owner/repo */
  repo: string;
  number: number;
  title: string;
  /** GitHub login of the pull request's author. */
  author: string;
  headRef: string;
  /** The commit the pull request is at; the arm is bound to it. */
  headSha: string;
  baseRef: string;
  method: 'merge' | 'squash' | 'rebase';
  /** "squashed into one commit" */
  methodWords: string;
  autoMergeOff: boolean;
}

export interface TeamApi {
  state: () => Promise<TeamState>;
  configGet: () => Promise<TeamConfigView>;
  configSet: (config: TeamConfigInput) => Promise<TeamConfigView>;
  create: (input: TeamCreateInput) => Promise<TeamConfigView>;
  invite: () => Promise<InviteView>;
  join: (input: TeamJoinInput) => Promise<TeamConfigView>;
  leave: () => Promise<TeamConfigView>;
  approveJoin: (joinId: string) => Promise<TeamConfigView>;
  declineJoin: (joinId: string) => Promise<TeamConfigView>;
  removeMember: (memberId: string) => Promise<TeamConfigView>;
  /** Admin: the member's claimed GitHub login is theirs (GitHub facts about it then show as theirs). */
  confirmGithub: (memberId: string) => Promise<TeamConfigView>;
  clearGaps: () => Promise<void>;
  mergePlan: (cardId: string) => Promise<MergePlan>;
  /** Arms auto-merge at `headSha`, the commit the sheet showed. */
  mergeWhenGreen: (cardId: string, headSha: string) => Promise<string>;
  send: (draftId: string, note?: string) => Promise<void>;
  dismissDraft: (draftId: string) => Promise<void>;
  dismissCard: (cardId: string) => Promise<void>;
  pull: (cardId: string) => Promise<string>;
  queue: (cardId: string, paneId: string, when: 'idle' | 'now') => Promise<TeamJob>;
  /** Starts the agent on a `needs_start` update, in a visible pane (desktop only). */
  startJob: (jobId: string) => Promise<void>;
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
  create: (input) => call('team_create', { input }),
  invite: () => call('team_invite'),
  join: (input) => call('team_join', { input }),
  leave: () => call('team_leave'),
  approveJoin: (joinId) => call('team_join_approve', { joinId }),
  declineJoin: (joinId) => call('team_join_decline', { joinId }),
  removeMember: (memberId) => call('team_member_remove', { memberId }),
  confirmGithub: (memberId) => call('team_member_confirm_github', { memberId }),
  clearGaps: () => call('team_gaps_clear'),
  mergePlan: (cardId) => call('team_merge_plan', { cardId }),
  mergeWhenGreen: (cardId, headSha) => call('team_merge_when_green', { cardId, headSha }),
  send: (draftId, note) => call('team_send', { draftId, note: note ?? null }),
  dismissDraft: (draftId) => call('team_draft_dismiss', { draftId }),
  dismissCard: (cardId) => call('team_card_dismiss', { cardId }),
  pull: (cardId) => call('team_pull', { cardId }),
  queue: (cardId, paneId, when) => call('team_queue', { cardId, paneId, when }),
  startJob: (jobId) => call('team_job_start', { jobId }),
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
  if (pr && event.kind === 'pr_merged') {
    const base = event.subject.base;
    return base ? `${who}'s #${pr.number} merged into ${base}` : `#${pr.number} merged`;
  }
  if (pr && event.kind === 'pr_closed') return `#${pr.number} closed without merging`;
  if (event.kind === 'please_review') {
    const what = pr ? `#${pr.number}` : (event.subject.branch ?? 'a branch');
    return `${who} asks for a review of ${what}`;
  }
  if (event.kind === 'note') return `${who} left a note`;
  if (event.kind === 'ci_failed' || event.kind === 'ci_passed') {
    const verb = event.kind === 'ci_failed' ? 'failed' : 'passed';
    const on = pr ? `#${pr.number}` : (event.subject.branch ?? 'a branch');
    return `Checks ${verb} on ${on}`;
  }
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
  const failed = event.subject.ci?.failedJobs ?? [];
  if (event.kind === 'ci_failed' && failed.length > 0) parts.push(`${failed.join(', ')} failed`);
  return parts.filter(Boolean).join(' · ');
}

/** Merge when green applies to a GitHub pull request only. */
export function isGithubPr(event: TeamEvent): boolean {
  return !!event.subject.pr && event.project.key.startsWith('github.com/');
}

/** A GitHub fact (merged, closed, checks) has no commit to pull, so the pull/queue note does not apply. */
const hasNothingToPull = (event: TeamEvent): boolean => !event.subject.commit;

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
  if (pr && (e.kind === 'pr_opened' || e.kind === 'pr_ready')) return `PR #${pr.number} is open${pr.title ? ` · ${pr.title}` : ''}`;
  if (e.kind === 'please_review') return `Ask for a review of ${pr ? `#${pr.number}` : (e.subject.branch ?? 'a branch')}`;
  if (e.kind === 'note') return 'A note for the team';
  const who = e.actor.agent?.label ?? 'You';
  const stats = statsText(e.subject.stats);
  return `${who} pushed ${e.subject.branch ?? 'a branch'}${stats ? ` · ${stats}` : ''}`;
}

/** Which actions a teammate card shows. At most two buttons, plus Dismiss. */
export interface CardActions {
  primary: 'pull' | 'queue' | null;
  /** "Merge when green": an open pull request on a card this app learned from GitHub itself (never a teammate's). */
  mergeWhenGreen: boolean;
  /** Why nothing is offered (a plain sentence), when it isn't. */
  note: string | null;
}

export function cardActions(card: TeamCard): CardActions {
  // A teammate's card can name any repository, number and title, so only GitHub's own facts offer it.
  const mergeWhenGreen = card.event.source === 'github' && isGithubPr(card.event) && card.event.subject.pr?.state === 'open';
  if (!card.local && hasNothingToPull(card.event)) return { primary: null, mergeWhenGreen, note: null };
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
    case 'needs_start':
      return `${plural(job.conflicts.length, 'conflict')} to resolve: ${job.theirBranch} into ${job.into}`;
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
