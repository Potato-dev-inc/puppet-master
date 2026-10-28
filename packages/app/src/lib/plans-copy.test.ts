import { describe, expect, it } from 'vitest';
import type { PlanRow } from './plans-report';
import { NO_TURNS_TEXT, PLANS_CSV_HEADER, csvField, planCsvCells, plansCsv, plansSummaryText, planSummaryLine } from './plans-copy';

function row(over: Partial<PlanRow>): PlanRow {
  return {
    agentId: 'claude',
    agentLabel: 'Claude Code',
    accountId: 'acct-work',
    accountLabel: 'Work',
    turns: 22,
    busyMs: (9 * 60 + 11) * 60_000,
    files: { knownTurns: 20, changed: 47, added: 340, deleted: 45 },
    merged: 2,
    limits: { hits: 0, limitedMs: 0 },
    ...over,
  };
}

describe('csvField', () => {
  it('leaves plain text and numbers alone', () => {
    expect(csvField('Claude Code')).toBe('Claude Code');
    expect(csvField(42)).toBe('42');
    expect(csvField(0)).toBe('0');
  });

  it('quotes a field with a comma, a quote or a line break, and doubles inner quotes', () => {
    expect(csvField('Work, main')).toBe('"Work, main"');
    expect(csvField('say "hi"')).toBe('"say ""hi"""');
    expect(csvField('line one\nline two')).toBe('"line one\nline two"');
    expect(csvField('a\r\nb')).toBe('"a\r\nb"');
  });
});

describe('plansCsv', () => {
  it('writes the header in the agreed column order', () => {
    expect(PLANS_CSV_HEADER).toEqual([
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
    ]);
    expect(plansCsv([]).split('\r\n')[0]).toBe(PLANS_CSV_HEADER.join(','));
  });

  it('puts raw numbers in each column, with no units', () => {
    const rows = [row({ limits: { hits: 3, limitedMs: 7 * 3_600_000 } })];
    expect(plansCsv(rows).split('\r\n')[1]).toBe('Claude Code,Work,22,551,47,340,45,2,3,7');
  });

  it('rounds busy minutes and hours waiting', () => {
    expect(planCsvCells(row({ busyMs: 90_000, limits: { hits: 1, limitedMs: 90 * 60_000 } }))).toEqual([
      'Claude Code',
      'Work',
      22,
      2,
      47,
      340,
      45,
      2,
      1,
      1.5,
    ]);
    expect(planCsvCells(row({ limits: { hits: 1, limitedMs: 25 * 60_000 } }))[9]).toBe(0.42);
  });

  it('escapes names that hold commas and quotes, and keeps one line per plan', () => {
    const rows = [row({ agentLabel: 'Alpha, "beta"', accountLabel: 'Team\nline' }), row({ agentId: 'nimbus', agentLabel: 'Nimbus', accountLabel: 'Default account', turns: 5 })];
    const lines = plansCsv(rows).split('\r\n');
    expect(lines).toHaveLength(3);
    expect(lines[1].startsWith('"Alpha, ""beta""","Team\nline",22,')).toBe(true);
    expect(lines[2]).toBe('Nimbus,Default account,5,551,47,340,45,2,0,0');
  });

  it('gives the header only for an empty month', () => {
    expect(plansCsv([])).toBe(PLANS_CSV_HEADER.join(','));
  });
});

describe('planSummaryLine', () => {
  it('reads as one plain sentence per plan', () => {
    expect(planSummaryLine(row({}))).toBe('Claude Code · Work: 22 turns, 9 h 11 min, 47 files, no limits hit.');
  });

  it('says when a limit was hit and how long it waited', () => {
    expect(planSummaryLine(row({ limits: { hits: 3, limitedMs: 7 * 3_600_000 } }))).toBe(
      'Claude Code · Work: 22 turns, 9 h 11 min, 47 files, hit its limit 3 times · 7 h waiting.',
    );
  });

  it('uses singular words for one turn and one file, and says when files were not reported', () => {
    expect(planSummaryLine(row({ turns: 1, files: { knownTurns: 1, changed: 1, added: 2, deleted: 0 } }))).toContain(
      '1 turn, 9 h 11 min, 1 file,',
    );
    expect(planSummaryLine(row({ files: { knownTurns: 2, changed: 0, added: 0, deleted: 0 } }))).toContain('no files changed,');
    expect(planSummaryLine(row({ files: { knownTurns: 0, changed: 0, added: 0, deleted: 0 } }))).toContain('files not reported,');
  });
});

describe('plansSummaryText', () => {
  it('puts one line per plan', () => {
    const text = plansSummaryText([row({}), row({ agentId: 'nimbus', agentLabel: 'Nimbus', accountLabel: 'Default account', turns: 5, busyMs: 90_000 })]);
    expect(text.split('\n')).toEqual([
      'Claude Code · Work: 22 turns, 9 h 11 min, 47 files, no limits hit.',
      'Nimbus · Default account: 5 turns, 2 min, 47 files, no limits hit.',
    ]);
  });

  it('says there are no turns for an empty month', () => {
    expect(plansSummaryText([])).toBe(NO_TURNS_TEXT);
    expect(NO_TURNS_TEXT).toBe('No turns this month yet.');
  });
});
