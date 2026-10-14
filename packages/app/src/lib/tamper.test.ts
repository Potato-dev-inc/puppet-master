import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { blockingFindings, cardLines, receiptLine, tamperCheck, type TamperFinding, type TamperReport } from './tamper';

function finding(over: Partial<TamperFinding>): TamperFinding {
  return {
    kind: 'skip_added',
    severity: 'blocks',
    file: 'cart.test.ts',
    line: 9,
    before: null,
    after: "it.skip('handles 0.005', () => {",
    summary: 'Added .skip to "handles 0.005" in cart.test.ts',
    count: 1,
    byPerson: false,
    inNewFile: false,
    ...over,
  };
}

function report(findings: TamperFinding[]): TamperReport {
  return {
    snapshotId: 'start',
    checkedAtMs: 1,
    blocks: findings.some((f) => f.severity === 'blocks'),
    findings,
    headline: findings.length ? 'Tests were changed' : 'No test changes',
    filesChanged: findings.length,
    filesChecked: 3,
    partial: false,
  };
}

describe('tamperCheck', () => {
  beforeEach(() => invoke.mockReset());

  it('calls the read-only command with the counts when it has them', async () => {
    invoke.mockResolvedValue(report([]));
    await tamperCheck('D:/proj', 'start', { total: 9, skipped: 1 });
    expect(invoke).toHaveBeenCalledWith('tamper_check', { project: 'D:/proj', since: 'start', total: 9, skipped: 1 });
    await tamperCheck('D:/proj', 'start');
    expect(invoke).toHaveBeenLastCalledWith('tamper_check', { project: 'D:/proj', since: 'start', total: null, skipped: null });
  });
});

describe('card and receipt wording', () => {
  const notes = finding({ severity: 'note', kind: 'tests_changed', summary: 'Changed cart.test.ts (+2 −1)' });

  it('lists three sentences then how many more', () => {
    const many = [1, 2, 3, 4, 5].map((n) => finding({ summary: `change ${n}` }));
    expect(cardLines(report(many))).toEqual(['change 1', 'change 2', 'change 3', 'and 2 more changes']);
    expect(cardLines(report(many.slice(0, 4)))).toEqual(['change 1', 'change 2', 'change 3', 'and 1 more change']);
  });

  it('leaves notes off the card', () => {
    const r = report([finding({}), notes]);
    expect(blockingFindings(r)).toHaveLength(1);
    expect(cardLines(r)).toEqual(['Added .skip to "handles 0.005" in cart.test.ts']);
  });

  it('has no receipt line when nothing changed, and counts only blocking changes', () => {
    expect(receiptLine(report([]))).toBeNull();
    expect(receiptLine(report([finding({})]))).toBe('Test changes to check: 1');
    expect(receiptLine(report([finding({}), finding({})]))).toBe('Test changes to check: 2');
    expect(receiptLine(report([notes]))).toBe('Tests were changed');
  });
});
