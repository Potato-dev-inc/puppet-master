// The Receipt tab in plain words. The backend receipt (`RunReceipt`, `agent_runs/runtime/receipt.rs`) is
// snake_case JSON for tools; people read "Result: Finished, not verified", not `outcome: completed_unchecked`.
import { backendLabel } from '../../lib/standby';
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
  result_capture: 'Answer taken from',
  not_verified: 'Not verified',
  build_id: 'App build',
  standby_used: 'Warm agent',
  fallback: 'Ran headless instead',
  'fallback.from': 'Wanted',
  'fallback.to': 'Ran',
  'fallback.reason': 'Why',
  worktree: 'Isolated copy',
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
  in_progress: 'Still running',
};

const VALUES: Record<string, Record<string, string>> = {
  outcome: OUTCOME,
  read_only: { enforced: 'Yes', unenforced: 'Asked for, not enforced by this agent', off: 'No' },
  result_capture: { authoritative: 'The agent', inferred: 'The terminal screen', missing: 'Nothing captured' },
  'fallback.reason': { visible_failed: 'The visible pane did not start', no_visible_mode: 'This agent has no visible pane' },
  'worktree.state': { unreviewed: 'Not merged yet', merged: 'Merged', discarded: 'Discarded' },
};

/** Status of the run itself, as a person would say it. */
export function runStatusLabel(status: string): string {
  const known: Record<string, string> = {
    queued: 'Waiting to start',
    starting: 'Starting',
    running: 'Running',
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

/** Receipt rows with plain labels and values. An unknown field still shows (sentence case), never raw snake_case. */
export function readableReceipt(receipt: unknown): ReceiptLine[] {
  const lines = receiptRows(receipt).map(({ key, value }): ReceiptLine & { raw: string } => {
    const tail = key.includes('.') ? key.slice(key.lastIndexOf('.') + 1) : key;
    const label = LABELS[key] ?? sentence(tail.replace(/\[\d+\]$/, ''));
    const mapped = VALUES[key]?.[value];
    if (mapped) return { raw: key, key: label, value: mapped };
    if (key === 'backend') return { raw: key, key: label, value: backendLabel(value) };
    if (key === 'standby_used') return { raw: key, key: label, value: value === 'true' ? 'Used a ready one' : value };
    if (key === 'fallback.from' || key === 'fallback.to') return { raw: key, key: label, value: sentence(value) };
    if (key === 'not_verified') return { raw: key, key: label, value: value.split(', ').map(sentence).join('; ') };
    return { raw: key, key: label, value };
  });
  // The answer to "did it work" leads.
  const lead = lines.filter((line) => line.raw === 'outcome');
  return [...lead, ...lines.filter((line) => line.raw !== 'outcome')].map(({ key, value }) => ({ key, value }));
}
