import { describe, expect, it } from 'vitest';
import { EMPTY_CANARY, changeLine, checkedAgo, lastCheckLine } from './canary';
import { CLAUDE_TESTED, CLAUDE_UPDATED_GOOD, CODEX_CHANGED } from '../dev/canary-fixtures';

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

describe('changeLine', () => {
  it('is null before the first update', () => {
    expect(changeLine(CLAUDE_TESTED, 0)).toBeNull();
  });

  it('says both versions and how the new one got on, then when it updated', () => {
    const at = CLAUDE_UPDATED_GOOD.change!.at_ms;
    expect(changeLine(CLAUDE_UPDATED_GOOD, at + 3 * 3_600_000)).toBe(
      'Claude Code 2.3.0 \u2192 2.4.0: checked, all good. Updated 3 hours ago.',
    );
    expect(changeLine(CODEX_CHANGED, CODEX_CHANGED.change!.at_ms + 5_000)).toBe(
      'Codex 0.159.3 \u2192 0.162.1: screens changed. Puppet Master will ask you before acting. Updated just now.',
    );
  });

  it('leaves out the time when it was not recorded', () => {
    const agent = { ...CLAUDE_UPDATED_GOOD, change: { ...CLAUDE_UPDATED_GOOD.change!, at_ms: 0 } };
    expect(changeLine(agent, 99)).toBe('Claude Code 2.3.0 \u2192 2.4.0: checked, all good.');
  });

  it('a verified version has no chip and says it was checked here', () => {
    expect(CLAUDE_UPDATED_GOOD.chip).toBeNull();
    expect(CLAUDE_UPDATED_GOOD.verified).toBe(true);
    expect(CLAUDE_UPDATED_GOOD.plain).toContain('checked on this computer');
  });
});

it('the empty status flags nothing', () => {
  expect(EMPTY_CANARY.agents).toEqual([]);
  expect(EMPTY_CANARY.notices).toEqual([]);
});
