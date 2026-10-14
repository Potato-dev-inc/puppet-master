// "Test changes to check": whether an agent weakened the tests or the check config since a
// snapshot a mission took at its start. The `tamper_check` Tauri command (src-tauri/src/commands/tamper.rs)
// and `GET /tamper/check?project=&since=` on the bridge return the same report. Read-only.
import { invoke } from '@tauri-apps/api/core';

export type TamperKind =
  | 'skip_added'
  | 'focus_added'
  | 'tests_removed'
  | 'assertions_removed'
  | 'assertion_loosened'
  | 'snapshots_rewritten'
  | 'checks_changed'
  | 'fewer_tests_ran'
  | 'checks_silenced'
  | 'tests_changed'
  | 'cant_read';

/** `blocks` stops "done" and raises a card; `note` only shows in the timeline and on the receipt. */
export type TamperSeverity = 'blocks' | 'note';

export interface TamperFinding {
  kind: TamperKind;
  severity: TamperSeverity;
  /** Project-relative path with forward slashes. Empty for findings about the test runner's counts. */
  file: string;
  line: number | null;
  before: string | null;
  after: string | null;
  /** One plain sentence, ready for a card: `Added .skip to "handles 0.005" in cart.test.ts`. */
  summary: string;
  count: number;
  /** The file was edited by the person between rounds, so this is a note about them. */
  byPerson: boolean;
  inNewFile: boolean;
}

export interface TamperReport {
  snapshotId: string;
  checkedAtMs: number;
  /** At least one finding blocks "done". */
  blocks: boolean;
  findings: TamperFinding[];
  /** `Tests were changed`, `No test changes`, ... */
  headline: string;
  filesChanged: number;
  filesChecked: number;
  /** Some files were too big or not text, so only their size and hash were compared. */
  partial: boolean;
}

export interface TestCounts {
  total?: number;
  skipped?: number;
}

/** Compare a project with a snapshot taken earlier. Rejects with a plain sentence. */
export function tamperCheck(project: string, since: string, counts?: TestCounts): Promise<TamperReport> {
  return invoke<TamperReport>('tamper_check', { project, since, total: counts?.total ?? null, skipped: counts?.skipped ?? null });
}

/** The findings that stop "done", in the order the card lists them. */
export function blockingFindings(report: TamperReport): TamperFinding[] {
  return report.findings.filter((f) => f.severity === 'blocks');
}

/**
 * The card body: up to `max` sentences, then "and N more changes" (which opens the diff).
 * Never says "tamper", "snapshot" or an id.
 */
export function cardLines(report: TamperReport, max = 3): string[] {
  const blocking = blockingFindings(report);
  const shown = blocking.slice(0, max).map((f) => f.summary);
  const rest = blocking.length - shown.length;
  if (rest > 0) shown.push(rest === 1 ? 'and 1 more change' : `and ${rest} more changes`);
  return shown;
}

/** The one receipt line: nothing when nothing changed, else how many changes need a look. */
export function receiptLine(report: TamperReport): string | null {
  if (report.findings.length === 0) return null;
  const blocking = blockingFindings(report).length;
  if (blocking === 0) return report.headline;
  return blocking === 1 ? 'Test changes to check: 1' : `Test changes to check: ${blocking}`;
}
