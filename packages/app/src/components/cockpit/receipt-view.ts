// The Receipt tab in plain words. The backend receipt (`RunReceipt`, `agent_runs/runtime/receipt.rs`) is
// snake_case JSON for tools; people read "Result: Finished, not verified", not `outcome: completed_unchecked`.
import { backendLabel } from '../../lib/standby';
import { resetTimeText } from '../../lib/limit-time';
import { receiptRows } from './model';

export interface ReceiptLine {
  key: string;
  value: string;
}

const LABELS: Record<string, string> = {
  outcome: 'Result',
  backend: 'Agent',
  model: 'Model',
  read_only: 'Read-only',
  checks: 'Checks',
  result_capture: 'Answer read from',
  not_verified: 'Not verified',
  build_id: 'App build',
  standby_used: 'Started from',
  fallback: 'Ran in the background instead',
  run_mode: 'How it ran',
  'run_mode.mode': 'Mode',
  'run_mode.source': 'Chosen by',
  'fallback.from': 'Wanted',
  'fallback.to': 'Ran',
  'fallback.reason': 'Why',
  usage_limit: 'Usage limit',
  worktree: 'Separate copy',
  'worktree.path': 'Folder',
  'worktree.branch': 'Branch',
  'worktree.base': 'Based on',
  'worktree.state': 'State',
  'worktree.files_changed': 'Files changed',
  'worktree.additions': 'Lines added',
  'worktree.deletions': 'Lines removed',
};

const OUTCOME: Record<string, string> = {
  passed_checks: 'Passed its checks',
  completed_unchecked: 'Finished, not verified',
  failed_checks: 'Failed its checks',
  failed: 'Failed',
  cancelled: 'Cancelled',
  needs_input: 'Needs you',
  in_progress: 'Still working',
};

const VALUES: Record<string, Record<string, string>> = {
  outcome: OUTCOME,
  read_only: { enforced: 'Yes', unenforced: 'Asked for, not enforced by this agent', off: 'No' },
  result_capture: { authoritative: 'The agent', inferred: 'The screen', missing: 'Nothing captured' },
  'fallback.reason': {
    visible_failed: 'The visible pane did not start',
    no_visible_mode: 'This agent has no visible pane',
    not_verified: 'One-task mode is not verified for this agent',
  },
  'run_mode.mode': { one_task: 'One-task mode', visible: 'Visible pane' },
  'run_mode.source': { agent_setting: 'The agent setting' },
  'worktree.state': { unreviewed: 'Not merged yet', merged: 'Merged', discarded: 'Discarded' },
};

/** Status of the run itself, as a person would say it. */
export function runStatusLabel(status: string): string {
  const known: Record<string, string> = {
    queued: 'Waiting to start',
    starting: 'Starting',
    running: 'Working',
    waiting_input: 'Needs you',
    needs_input: 'Needs you',
    cancelling: 'Stopping',
    completed: 'Finished',
    failed: 'Failed',
    cancelled: 'Cancelled',
  };
  return known[status] ?? sentence(status);
}

function sentence(text: string): string {
  const spaced = text.replace(/[_-]+/g, ' ').replace(/\s+/g, ' ').trim();
  return spaced ? spaced.charAt(0).toUpperCase() + spaced.slice(1) : '—';
}

/**
 * The line for a run that hit a usage limit: "Stopped at a usage limit (resets 14:05)" once a person chose
 * to stop there, "Waiting at a usage limit (resets 14:05)" while the agent is still at it. `undefined` for
 * any other run. The reset is the moment when it could be read, else the text the screen printed.
 */
export function usageLimitLine(receipt: unknown, now: number = Date.now()): string | undefined {
  const limit = (receipt as { usage_limit?: { state?: string; reset_at_ms?: number; reset_text?: string } } | null | undefined)?.usage_limit;
  if (!limit || (limit.state !== 'stopped' && limit.state !== 'waiting')) return undefined;
  const resets =
    typeof limit.reset_at_ms === 'number' ? resetTimeText(limit.reset_at_ms, now) : limit.reset_text?.trim() || undefined;
  const lead = limit.state === 'stopped' ? 'Stopped at a usage limit' : 'Waiting at a usage limit';
  return resets ? `${lead} (resets ${resets})` : lead;
}

type RawLine = ReceiptLine & { raw: string };

function labelled(receipt: unknown, now: number): { rows: RawLine[]; limitLine: string | undefined } {
  const limitLine = usageLimitLine(receipt, now);
  const rows = receiptRows(receipt)
    // The limit's own fields fold into one line below.
    .filter(({ key }) => !(limitLine && key.startsWith('usage_limit.')))
    .map(({ key, value }): RawLine => {
      const tail = key.includes('.') ? key.slice(key.lastIndexOf('.') + 1) : key;
      const label = LABELS[key] ?? sentence(tail.replace(/\[\d+\]$/, ''));
      const mapped = VALUES[key]?.[value];
      if (mapped) return { raw: key, key: label, value: mapped };
      if (key === 'backend') return { raw: key, key: label, value: backendLabel(value) };
      if (key === 'standby_used') return { raw: key, key: label, value: value === 'true' ? 'A ready agent' : value };
      if (key === 'fallback.from' || key === 'fallback.to') return { raw: key, key: label, value: sentence(value) };
      if (key === 'not_verified') return { raw: key, key: label, value: value.split(', ').map(sentence).join('; ') };
      return { raw: key, key: label, value };
    });
  return { rows, limitLine };
}

/** Receipt rows with plain labels and values. An unknown field still shows (sentence case), never raw snake_case. */
export function readableReceipt(receipt: unknown, now: number = Date.now()): ReceiptLine[] {
  const { rows, limitLine } = labelled(receipt, now);
  // The answer to "did it work" leads, and says where it stopped when it ran out of usage.
  const lead = rows.filter((line) => line.raw === 'outcome').map((line) => (limitLine ? { ...line, value: limitLine } : line));
  return [...lead, ...rows.filter((line) => line.raw !== 'outcome')].map(({ key, value }) => ({ key, value }));
}

/** What a separate copy holds right now (the Changes tab's own numbers). */
export interface CopyTotals {
  files_changed: number;
  additions: number;
  deletions: number;
}

export interface ReceiptSummary {
  /** One plain sentence on what was checked. */
  checks: string;
  /** At most five rows, the ones a person reads. */
  facts: ReceiptLine[];
  /** Everything else, for support and curious people. */
  details: ReceiptLine[];
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

/** "3 files +14 −3". */
export function copyTotalsText(t: CopyTotals): string {
  return `${plural(t.files_changed, 'file')} +${t.additions} −${t.deletions}`;
}

/** The checks, in a sentence a person can read: "No checks are set up for this project." */
export function checksSentence(receipt: unknown): string {
  const checks = (receipt as { checks?: unknown } | null | undefined)?.checks;
  const text = typeof checks === 'string' ? checks.trim() : '';
  if (!text || text === 'none configured') return 'No checks are set up for this project, so nothing was verified.';
  const counted = /^(\d+)\/(\d+) passed$/.exec(text);
  if (counted) {
    const [, passed, total] = counted;
    return passed === total ? `All ${total} checks passed.` : `${passed} of ${total} checks passed.`;
  }
  return `${sentence(text)}.`;
}

function recordedTotals(receipt: unknown): CopyTotals | null {
  const w = (receipt as { worktree?: { files_changed?: unknown; additions?: unknown; deletions?: unknown } } | null | undefined)?.worktree;
  if (!w || typeof w.files_changed !== 'number') return null;
  return {
    files_changed: w.files_changed,
    additions: typeof w.additions === 'number' ? w.additions : 0,
    deletions: typeof w.deletions === 'number' ? w.deletions : 0,
  };
}

/** Receipt fields that are shown as a fact, in the checks sentence or replaced by live numbers. */
const NOT_IN_DETAILS = new Set([
  'outcome',
  'result_capture',
  'checks',
  'model',
  'worktree.state',
  'worktree.files_changed',
  'worktree.additions',
  'worktree.deletions',
]);

/**
 * The Receipt as a short summary: the checks, at most five facts and the rest under Details.
 *
 * File counts have ONE source: `copy` (what the separate copy holds right now, the same numbers the
 * Changes tab and its tab label show). The receipt's own snapshot of the counts is dropped, so the
 * page cannot show two different totals; one turn's own count lives in the Last turn line.
 */
export function receiptSummary(receipt: unknown, copy: CopyTotals | null, now: number = Date.now()): ReceiptSummary {
  const { rows, limitLine } = labelled(receipt, now);
  const by = (raw: string) => rows.find((row) => row.raw === raw);
  const facts: ReceiptLine[] = [];
  const outcome = by('outcome');
  if (outcome) facts.push({ key: 'Result', value: limitLine ?? outcome.value });
  const capture = by('result_capture');
  if (capture) facts.push({ key: capture.key, value: capture.value });
  const state = by('worktree.state');
  // The live numbers win; a copy that is gone (merged, discarded) has only what the receipt recorded.
  const totals = copy ?? recordedTotals(receipt);
  if (state || totals) {
    const stateWord = state?.value ?? 'Not merged yet';
    facts.push({ key: 'Separate copy', value: totals ? `${stateWord} · ${copyTotalsText(totals)} in all` : stateWord });
  }
  const model = by('model');
  if (model) facts.push({ key: model.key, value: model.value });
  const details = rows.filter((row) => !NOT_IN_DETAILS.has(row.raw)).map(({ key, value }) => ({ key, value }));
  return { checks: checksSentence(receipt), facts: facts.slice(0, 5), details };
}
