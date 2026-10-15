import { describe, expect, it } from 'vitest';
import { agePhrase, formatAge } from './inbox-age';

describe('formatAge: one short format on every card', () => {
  it.each([
    [0, 'now'],
    [4_999, 'now'],
    [5_000, '5s'],
    [42_000, '42s'],
    [59_999, '59s'],
    [60_000, '1m'],
    [125_000, '2m'],
    [59 * 60_000 + 59_000, '59m'],
    [3_600_000, '1h'],
    [3 * 3_600_000 + 12 * 60_000, '3h'],
    [26 * 3_600_000, '1d'],
    [-5_000, 'now'],
  ])('%i ms is %s', (ms, text) => {
    expect(formatAge(ms)).toBe(text);
  });

  it('never carries a prefix or a suffix word', () => {
    for (const ms of [0, 9_000, 130_000, 7_300_000, 200_000_000]) expect(formatAge(ms)).toMatch(/^(now|\d+[smhd])$/);
  });
});

describe('agePhrase: the same age for the tooltip and screen readers', () => {
  it('says it in words', () => {
    expect(agePhrase(2_000)).toBe('just now');
    expect(agePhrase(1_000 * 42)).toBe('42 seconds ago');
    expect(agePhrase(60_000)).toBe('1 minute ago');
    expect(agePhrase(125_000)).toBe('2 minutes ago');
    expect(agePhrase(3 * 3_600_000)).toBe('3 hours ago');
    expect(agePhrase(50 * 3_600_000)).toBe('2 days ago');
  });
});
