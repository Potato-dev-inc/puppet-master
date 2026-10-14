// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { readableReceipt, runStatusLabel, usageLimitLine } from './receipt-view';

const receipt = {
  backend: 'claude',
  model: 'claude-opus-5-5',
  read_only: 'off',
  checks: 'none configured',
  result_capture: 'inferred',
  not_verified: ['no checks configured', 'model not confirmed'],
  outcome: 'completed_unchecked',
  build_id: 'abc',
  standby_used: true,
  fallback: { from: 'visible', to: 'headless', reason: 'visible_failed' },
  worktree: {
    path: 'D:/x',
    branch: 'pm/a',
    base: 'master',
    state: 'unreviewed',
    files_changed: 2,
    additions: 61,
    deletions: 4,
  },
};

function valueOf(lines: { key: string; value: string }[], key: string): string | undefined {
  return lines.find((line) => line.key === key)?.value;
}

describe('readableReceipt', () => {
  it('turns a realistic receipt into plain labels and values', () => {
    const lines = readableReceipt(receipt);

    expect(valueOf(lines, 'Result')).toBe('Finished, not verified');
    expect(valueOf(lines, 'Agent')).toBe('Claude Code');
    expect(valueOf(lines, 'Read-only')).toBe('No');
    expect(valueOf(lines, 'Answer taken from')).toBe('The terminal screen');
    expect(valueOf(lines, 'Not verified')).toBe('No checks configured; Model not confirmed');
    expect(valueOf(lines, 'Warm agent')).toBe('Used a ready one');
    expect(valueOf(lines, 'Why')).toBe('The visible pane did not start');
    expect(valueOf(lines, 'State')).toBe('Not merged yet');
    expect(valueOf(lines, 'Lines added')).toBe('61');
  });

  it('never leaks underscores or dots into keys or the outcome, read-only and state values', () => {
    const lines = readableReceipt(receipt);
    const fieldKeys = ['Result', 'Read-only', 'State'];

    for (const line of lines) {
      expect(line.key).not.toContain('_');
      expect(line.key).not.toContain('.');
    }
    for (const key of fieldKeys) {
      const value = valueOf(lines, key);
      expect(value).toBeDefined();
      expect(value).not.toContain('_');
    }
  });

  it('shows an unknown top-level field in sentence case', () => {
    expect(readableReceipt({ weird_new_field: 'x' })).toEqual([{ key: 'Weird new field', value: 'x' }]);
  });

  it('shows a nested unknown field under the last segment in sentence case', () => {
    const lines = readableReceipt({ a_b: { c_d: 1 } });
    expect(lines).toHaveLength(1);
    expect(lines[0].key).toBe('C d');
    expect(lines[0].value).toBe('1');
  });
});

describe('runStatusLabel', () => {
  it('maps known run statuses to plain words', () => {
    expect(runStatusLabel('completed')).toBe('Finished');
    expect(runStatusLabel('waiting_input')).toBe('Needs you');
    expect(runStatusLabel('failed')).toBe('Failed');
    expect(runStatusLabel('cancelling')).toBe('Stopping');
  });

  it('turns an unknown status into sentence case', () => {
    expect(runStatusLabel('some_odd_state')).toBe('Some odd state');
  });

  it('says Working for a running run', () => {
    expect(runStatusLabel('running')).toBe('Working');
  });

  it('says Still working for an in-progress outcome', () => {
    expect(readableReceipt({ outcome: 'in_progress' })).toEqual([{ key: 'Result', value: 'Still working' }]);
  });
});

describe('readableReceipt null input', () => {
  it('returns no lines for a null receipt', () => {
    expect(readableReceipt(null)).toEqual([]);
  });
});

describe('readableReceipt order', () => {
  it('leads with the result', () => {
    const lines = readableReceipt({ backend: 'claude', outcome: 'passed_checks', checks: '2/2 passed' });
    expect(lines[0]).toEqual({ key: 'Result', value: 'Passed its checks' });
  });
});

describe('a run that hit a usage limit', () => {
  const noon = new Date(2026, 9, 15, 12, 0).getTime();
  const at = (h: number, m: number, day = 15) => new Date(2026, 9, day, h, m).getTime();
  const stopped = {
    ...receipt,
    outcome: 'cancelled',
    usage_limit: { state: 'stopped', window: 'session limit', reset_at_ms: at(14, 5), reset_text: '2:05pm (Asia/Taipei)' },
  };

  it('says "Stopped at a usage limit (resets 14:05)" as the result', () => {
    const lines = readableReceipt(stopped, noon);
    expect(valueOf(lines, 'Result')).toBe('Stopped at a usage limit (resets 14:05)');
    // The limit's own fields fold into that line; nothing raw is left behind.
    expect(lines.some((line) => /usage_limit|reset_at/i.test(line.key))).toBe(false);
    expect(lines.some((line) => line.value === String(at(14, 5)))).toBe(false);
  });

  it('shows the day when the reset is not today', () => {
    expect(usageLimitLine({ usage_limit: { state: 'stopped', reset_at_ms: at(17, 0, 18) } }, noon)).toBe(
      'Stopped at a usage limit (resets Oct 18 17:00)',
    );
  });

  it('falls back to the text the screen printed, then to no time at all', () => {
    expect(usageLimitLine({ usage_limit: { state: 'stopped', reset_text: '5pm (Asia/Taipei)' } }, noon)).toBe(
      'Stopped at a usage limit (resets 5pm (Asia/Taipei))',
    );
    expect(usageLimitLine({ usage_limit: { state: 'stopped' } }, noon)).toBe('Stopped at a usage limit');
  });

  it('a run still at its limit says it is waiting', () => {
    expect(usageLimitLine({ usage_limit: { state: 'waiting', reset_at_ms: at(14, 5) } }, noon)).toBe('Waiting at a usage limit (resets 14:05)');
  });

  it('any other run is untouched', () => {
    expect(usageLimitLine(receipt, noon)).toBeUndefined();
    expect(usageLimitLine(null, noon)).toBeUndefined();
    expect(usageLimitLine({ usage_limit: { state: 'unknown' } }, noon)).toBeUndefined();
    expect(valueOf(readableReceipt(receipt, noon), 'Result')).toBe('Finished, not verified');
  });
});
