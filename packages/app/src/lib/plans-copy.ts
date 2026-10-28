// "Copy as CSV" and "Copy as text" for "Your plans this month". Pure: rows in, string out.
// CSV numbers are raw (no units). Text is one plain sentence per plan.
import { formatBusy, limitsText, type PlanRow } from './plans-report';

export const PLANS_CSV_HEADER = [
  'Plan',
  'Account',
  'Turns',
  'Busy minutes',
  'Files',
  'Lines added',
  'Lines removed',
  'Merged',
  'Limit hits',
  'Hours waiting',
];

export const NO_TURNS_TEXT = 'No turns this month yet.';

const MIN = 60_000;
const HOUR = 60 * MIN;

/** A text field that a spreadsheet could read as a formula: starts with = + - @ tab or carriage return. */
const FORMULA_START = /^[=+\-@\t\r]/;

/**
 * One CSV field. Numbers are written as they are. A text field that could run as a formula gets a leading
 * single quote first. Then a field with a comma, a quote or a line break is quoted, and inner quotes are doubled (RFC 4180).
 */
export function csvField(value: string | number): string {
  if (typeof value === 'number') return String(value);
  const text = FORMULA_START.test(value) ? `'${value}` : value;
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** The cells of one plan, in `PLANS_CSV_HEADER` order. Minutes and hours are rounded, hours to two decimals. */
export function planCsvCells(row: PlanRow): (string | number)[] {
  return [
    row.agentLabel,
    row.accountLabel,
    row.turns,
    Math.round(row.busyMs / MIN),
    row.files.changed,
    row.files.added,
    row.files.deleted,
    row.merged,
    row.limits.hits,
    Math.round((row.limits.limitedMs / HOUR) * 100) / 100,
  ];
}

/** Header line, then one line per plan. CRLF line ends. An empty month gives the header only. */
export function plansCsv(rows: readonly PlanRow[]): string {
  const lines = [PLANS_CSV_HEADER, ...rows.map(planCsvCells)];
  return lines.map((cells) => cells.map(csvField).join(',')).join('\r\n');
}

/** "Claude Code · Work: 22 turns, 9 h 11 min, 47 files, no limits hit." */
export function planSummaryLine(row: PlanRow): string {
  const turns = row.turns === 1 ? '1 turn' : `${row.turns} turns`;
  const { changed, knownTurns } = row.files;
  const files =
    knownTurns === 0 ? 'files not reported' : changed === 0 ? 'no files changed' : `${changed} ${changed === 1 ? 'file' : 'files'}`;
  const limits = limitsText(row.limits);
  const limitsPhrase = limits.charAt(0).toLowerCase() + limits.slice(1);
  return `${row.agentLabel} · ${row.accountLabel}: ${turns}, ${formatBusy(row.busyMs)}, ${files}, ${limitsPhrase}.`;
}

/** One summary line per plan, joined with new lines. An empty month says so in one line. */
export function plansSummaryText(rows: readonly PlanRow[]): string {
  if (rows.length === 0) return NO_TURNS_TEXT;
  return rows.map(planSummaryLine).join('\n');
}
