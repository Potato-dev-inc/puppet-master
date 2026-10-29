import { afterEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  AHEAD_TITLE,
  type ScoreRate,
  type ScoreRow,
  brokeTestsText,
  detailLine,
  notEnoughText,
  percentText,
  perMergedText,
  rateText,
  sampleHint,
  scoresApi,
  windowLabel,
} from './agent-scores';

function rate(count: number, total: number, sample: ScoreRate['sample']): ScoreRate {
  return { count, total, sample, low: 0, high: 1 };
}

function row(over: Partial<ScoreRow> = {}): ScoreRow {
  const empty = rate(0, 0, 'none');
  return {
    agentId: 'a',
    agentLabel: 'Agent A',
    accountLabel: 'Work account',
    turns: 0,
    enough: true,
    endedOk: empty,
    checksGreen: empty,
    merged: empty,
    undone: empty,
    askedYou: empty,
    waitingReview: 0,
    brokeTests: 0,
    ahead: [],
    ...over,
  };
}

afterEach(() => {
  invoke.mockReset();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('rateText', () => {
  it('shows "x of y" for early and steady samples', () => {
    expect(rateText(rate(5, 5, 'early'))).toBe('5 of 5');
    expect(rateText(rate(8, 10, 'steady'))).toBe('8 of 10');
  });

  it('shows a dash when there are too few runs', () => {
    expect(rateText(rate(0, 0, 'none'))).toBe('-');
    expect(rateText(rate(3, 4, 'not_enough'))).toBe('-');
  });

  it('never shows more than the total', () => {
    expect(rateText(rate(12, 10, 'steady'))).toBe('10 of 10');
  });
});

describe('percentText', () => {
  it('shows a percent only for a steady sample', () => {
    expect(percentText(rate(8, 10, 'steady'))).toBe('80%');
    expect(percentText(rate(3, 3, 'early'))).toBeNull();
    expect(percentText(rate(0, 0, 'none'))).toBeNull();
  });
});

describe('sampleHint', () => {
  it('explains a dash with the number it needs', () => {
    expect(sampleHint(rate(3, 3, 'not_enough'), 5)).toBe('Only 3 so far. A number needs 5.');
    expect(sampleHint(rate(0, 0, 'none'), 5)).toBe('Only 0 so far. A number needs 5.');
  });

  it('has nothing to say once there is a number', () => {
    expect(sampleHint(rate(5, 5, 'early'), 5)).toBeNull();
    expect(sampleHint(rate(8, 10, 'steady'), 5)).toBeNull();
  });
});

describe('perMergedText', () => {
  it('reads turns and working time', () => {
    expect(perMergedText({ runs: 3, turnsMedian: 2, busyMsMedian: 14 * 60_000 })).toBe('2 turns · 14 min');
    expect(perMergedText({ runs: 3, turnsMedian: 1, busyMsMedian: 3 * 3_600_000 })).toBe('1 turn · 3 h');
  });

  it('is empty when there is no typical value', () => {
    expect(perMergedText(null)).toBeNull();
    expect(perMergedText(undefined)).toBeNull();
  });
});

describe('notEnoughText', () => {
  it('says how many turns there are so far', () => {
    expect(notEnoughText(row({ turns: 3 }))).toBe('Not enough runs yet. 3 turns so far.');
    expect(notEnoughText(row({ turns: 1 }))).toBe('Not enough runs yet. 1 turn so far.');
  });
});

describe('detailLine', () => {
  it('joins the typical turn, lines and waiting parts', () => {
    expect(
      detailLine({ turnMsMedian: 4 * 60_000, linesMedian: 38, waitingReview: 2 }),
    ).toBe('Typical turn 4 min · 38 lines · 2 waiting for review');
  });

  it('leaves out parts without a value', () => {
    expect(detailLine({ turnMsMedian: null, linesMedian: null, waitingReview: 0 })).toBe('');
    expect(detailLine({ turnMsMedian: 60_000, linesMedian: null, waitingReview: 0 })).toBe('Typical turn 1 min');
    expect(detailLine({ turnMsMedian: null, linesMedian: 1, waitingReview: 1 })).toBe('1 line · 1 waiting for review');
  });
});

describe('brokeTestsText', () => {
  it('says how many times the agent broke the tests, with singular and plural', () => {
    expect(brokeTestsText(row({ brokeTests: 2 }))).toBe('Broke the tests 2 times');
    expect(brokeTestsText(row({ brokeTests: 1 }))).toBe('Broke the tests 1 time');
  });

  it('is empty when the count is 0', () => {
    expect(brokeTestsText(row({ brokeTests: 0 }))).toBeNull();
  });

  it('is empty for a missing or negative count', () => {
    expect(brokeTestsText({ brokeTests: undefined as unknown as number })).toBeNull();
    expect(brokeTestsText(row({ brokeTests: -1 }))).toBeNull();
  });
});

describe('windowLabel', () => {
  it('names the three windows', () => {
    expect(windowLabel(30)).toBe('Last 30 days');
    expect(windowLabel(90)).toBe('Last 90 days');
    expect(windowLabel(null)).toBe('All time');
    expect(windowLabel(undefined)).toBe('All time');
  });
});

describe('AHEAD_TITLE', () => {
  it('is the plain sentence of the spec', () => {
    expect(AHEAD_TITLE).toBe('With this many runs the gap is unlikely to be luck.');
  });
});

describe('scoresApi.report', () => {
  it('refuses outside Tauri with a plain sentence', async () => {
    await expect(scoresApi.report()).rejects.toThrow('Open Puppet Master on your computer to see your scores.');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('calls agent_scores with null for missing arguments', async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({ scope: 'all' });
    await scoresApi.report();
    expect(invoke).toHaveBeenLastCalledWith('agent_scores', { project: null, days: null });
  });

  it('passes the project and the window through', async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({ scope: 'project' });
    await scoresApi.report('abc123def456', 30);
    expect(invoke).toHaveBeenLastCalledWith('agent_scores', { project: 'abc123def456', days: 30 });
    await scoresApi.report(undefined, null);
    expect(invoke).toHaveBeenLastCalledWith('agent_scores', { project: null, days: null });
  });
});
