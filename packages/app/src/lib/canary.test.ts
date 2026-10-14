import { describe, expect, it } from 'vitest';
import { EMPTY_CANARY, checkedAgo, lastCheckLine } from './canary';
import { CLAUDE_TESTED } from '../dev/canary-fixtures';

describe('checkedAgo', () => {
  const now = 10_000_000;
  it('says just now, minutes, hours and days in plain words', () => {
    expect(checkedAgo(now - 5_000, now)).toBe('just now');
    expect(checkedAgo(now - 60_000, now)).toBe('1 minute ago');
    expect(checkedAgo(now - 12 * 60_000, now)).toBe('12 minutes ago');
    expect(checkedAgo(now - 3 * 3_600_000, now)).toBe('3 hours ago');
    expect(checkedAgo(now - 2 * 86_400_000, now)).toBe('2 days ago');
  });
});

describe('lastCheckLine', () => {
  it('is null before any check and joins the summary with when it ran', () => {
    expect(lastCheckLine({ ...CLAUDE_TESTED, last: null }, 0)).toBeNull();
    const line = lastCheckLine(CLAUDE_TESTED, CLAUDE_TESTED.last!.at_ms + 12 * 60_000);
    expect(line).toBe('Claude Code started and reached its input box. Nothing was sent to it. Checked 12 minutes ago.');
  });
});

it('the empty status flags nothing', () => {
  expect(EMPTY_CANARY.agents).toEqual([]);
  expect(EMPTY_CANARY.notices).toEqual([]);
});
