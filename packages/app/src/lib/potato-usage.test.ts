// The Usage page's words and bars: money from nano-dollars, short tokens, bar widths that stop at 100%, the limit warnings,
// the day names, and the fake transport the page tests use.
import { describe, expect, it } from 'vitest';
import { barPercent, dayLabel, fakeUsageTransport, limitPercent, limitWord, moneyText, tokenText, usageError, type UsageReport } from './potato-usage';

describe('money and tokens in words', () => {
  it('turns nano-dollars into dollars with two decimals, and a few cents into "Under $0.01"', () => {
    expect(moneyText(0)).toBe('$0.00');
    expect(moneyText(1_000_000_000)).toBe('$1.00');
    expect(moneyText(250_000_000)).toBe('$0.25');
    expect(moneyText(1_234_567_890)).toBe('$1.23');
    expect(moneyText(300_000)).toBe('Under $0.01');
    expect(moneyText(-5)).toBe('$0.00');
  });

  it('shows tokens in short words', () => {
    expect([tokenText(0), tokenText(812), tokenText(4_200), tokenText(1_300_000)]).toEqual(['0', '812', '4.2k', '1.3M']);
  });
});

describe('bars and limits', () => {
  it('a bar is a share of its top, never more than the whole and never negative', () => {
    expect(barPercent(50, 200)).toBe(25);
    expect(barPercent(500, 200)).toBe(100);
    expect(barPercent(-1, 200)).toBe(0);
    expect(barPercent(5, 0), 'no top: nothing to show').toBe(0);
  });

  it('today against the daily limit, in percent of the cents', () => {
    expect(limitPercent(250_000_000, 50)).toBe(50);
    expect(limitPercent(600_000_000, 50)).toBe(100);
  });

  it('warns at 80% and says the limit is reached at 100%', () => {
    expect(limitWord(79)).toBe('');
    expect(limitWord(80)).toBe('Close to the limit.');
    expect(limitWord(100)).toBe('Limit reached. It stops until tomorrow.');
  });
});

describe('day names', () => {
  it('today, yesterday, then the short date', () => {
    expect(dayLabel('2026-11-01', '2026-11-01')).toBe('Today');
    expect(dayLabel('2026-10-31', '2026-11-01')).toBe('Yesterday');
    expect(dayLabel('2026-10-30', '2026-11-01')).toBe('Oct 30');
    expect(dayLabel('2026-03-01', '2026-03-01')).toBe('Today');
    expect(dayLabel('2026-02-28', '2026-03-01')).toBe('Yesterday');
  });
});

describe('the fake transport and the read errors', () => {
  const report: UsageReport = {
    today: '2026-11-01',
    limitCents: 50,
    sandboxActionsPerDay: 500,
    total: { messages: 0, tokens: 0, costNano: 0, sandboxActions: 0 },
    todayTotal: { messages: 0, tokens: 0, costNano: 0, sandboxActions: 0 },
    days: [],
    bots: [],
  };

  it('answers with the report and counts the asks', async () => {
    const fake = fakeUsageTransport(report);
    expect(fake.asked).toBe(0);
    await expect(fake.usage()).resolves.toEqual(report);
    expect(fake.asked).toBe(1);
  });

  it('fails with the plain sentence, and the code is dropped', async () => {
    const fake = fakeUsageTransport('the usage file is damaged.');
    const error = await fake.usage().catch((e: unknown) => e);
    expect(usageError(error)).toBe('the usage file is damaged.');
    expect(usageError(new Error('POTATO_NOT_READY: Potato Bot is not ready yet.'))).toBe('Potato Bot is not ready yet.');
    expect(usageError({})).toBe('Could not read the usage.');
  });
});
