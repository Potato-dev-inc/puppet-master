// The Review Desk harness scenarios are registered, and the mocked `review_desk_*` commands answer the shapes the
// desk reads. The fixture counts match the list they describe.
import { describe, expect, it } from 'vitest';
import { REVIEW_DESK_EMPTY, REVIEW_DESK_FULL, reviewDeskMock } from './review-desk-fixtures';
import { scenarios } from './scenarios';

describe('review desk scenarios', () => {
  it('are registered under their own names, on the cockpit', () => {
    for (const name of ['review-desk', 'review-desk-light', 'review-desk-empty']) {
      expect(scenarios[name]?.name).toBe(name);
      expect(scenarios[name]?.route).toBe('cockpit');
    }
    expect(scenarios['review-desk-light']?.theme).toBe('light');
    expect(scenarios['review-desk-empty']?.reviewDesk?.desk?.items).toEqual([]);
  });

  it('the full fixture counts its own rows: two high risk, one check first, two low risk, riskiest first', () => {
    const levels = REVIEW_DESK_FULL.items.map((i) => i.level);
    expect({
      total: levels.length,
      high: levels.filter((l) => l === 'high').length,
      check: levels.filter((l) => l === 'check').length,
      low: levels.filter((l) => l === 'low').length,
    }).toEqual(REVIEW_DESK_FULL.counts);
    const tiers = REVIEW_DESK_FULL.items.map((i) => i.tier);
    expect(tiers).toEqual([...tiers].sort((a, b) => a - b));
  });

  it('the mock answers the list and the count, and leaves other commands alone', () => {
    expect(reviewDeskMock(undefined, 'review_desk_list', {}).value).toBe(REVIEW_DESK_FULL);
    expect(reviewDeskMock({ desk: REVIEW_DESK_EMPTY }, 'review_desk_count', {}).value).toBe(0);
    expect(reviewDeskMock(undefined, 'night_state', {}).handled).toBe(false);
  });

  it('Ask to fix answers the pane of the row it was given', () => {
    const first = REVIEW_DESK_FULL.items[0];
    expect(reviewDeskMock(undefined, 'review_desk_fix', { key: first.key, fingerprint: first.fingerprint, note: '' }).value).toBe(
      first.fixPaneId,
    );
  });
});
