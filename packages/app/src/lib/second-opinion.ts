// "Second opinion": ask another agent to review what this one just did (Rust: second_opinion).
// Own file so it adds nothing to the shared cockpit-api.ts. Every call needs the desktop shell.
import { invoke } from '@tauri-apps/api/core';
import type { ReviewFinding } from '../components/cockpit/review-diff';

export type { ReviewFinding };

/** Where the reviewed diff came from, best first. */
export type DiffSource = 'worktree' | 'checkpoint' | 'git';

export interface ReviewerEntry {
  id: string;
  label: string;
  /** A warm worker is waiting, so the review starts at once. */
  ready: boolean;
  /** The agent that wrote the code (left out of the list unless the user asks). */
  same: boolean;
}

export interface ReviewStart {
  paneId: string;
  handle: string;
  project: string;
  reviewer: string;
  reviewerLabel: string;
  source: DiffSource;
  sourceLabel: string;
  /** The exact unified diff the reviewer was given. */
  patch: string;
  patchTruncated: boolean;
  files: number;
  additions: number;
  deletions: number;
  /** Display name of the original agent ("Claude Code"). */
  authorLabel: string;
}

export type VerdictLevel = 'approve' | 'concerns' | 'changes' | 'unknown';

export interface ParsedReview {
  findings: ReviewFinding[];
  verdict: { level: VerdictLevel; text: string } | null;
  /** The reviewer's own words when nothing could be parsed. */
  unstructured: string | null;
}

export interface ReviewPoll {
  state: 'running' | 'done' | 'failed' | 'needs_input';
  review: ParsedReview | null;
  error: string | null;
}

export interface SentFollowup {
  route: 'run' | 'pane';
  target: string;
}

export interface SecondOpinionApi {
  agents: (paneId: string) => Promise<ReviewerEntry[]>;
  start: (paneId: string, reviewer: string) => Promise<ReviewStart>;
  poll: (project: string, handle: string) => Promise<ReviewPoll>;
  cancel: (project: string, handle: string) => Promise<void>;
  send: (paneId: string, file: string, line: number | null, text: string) => Promise<SentFollowup>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

/** Errors from the backend read `CODE: message`; show only the message. */
export function plainError(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  return raw.replace(/^[A-Z_]{3,}:\s*/, '');
}

export const secondOpinionApi: SecondOpinionApi = {
  agents: (paneId) => call('second_opinion_agents', { paneId }),
  start: (paneId, reviewer) => call('second_opinion_start', { paneId, reviewer }),
  poll: (project, handle) => call('second_opinion_poll', { project, handle }),
  cancel: (project, handle) => call('second_opinion_cancel', { project, handle }),
  send: (paneId, file, line, text) => call('second_opinion_send', { paneId, file, line, text }),
};
