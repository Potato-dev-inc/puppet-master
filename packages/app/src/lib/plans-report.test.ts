import { describe, expect, it } from 'vitest';
import { filesText, formatBusy, formatWaiting, limitsText, mergedText, neighbour, plansApi } from './plans-report';

describe('formatBusy', () => {
  it('speaks in minutes and hours', () => {
    expect(formatBusy(0)).toBe('0 min');
    expect(formatBusy(20_000)).toBe('under 1 min');
    expect(formatBusy(42 * 60_000)).toBe('42 min');
    expect(formatBusy(3 * 3_600_000)).toBe('3 h');
    expect(formatBusy((3 * 60 + 12) * 60_000)).toBe('3 h 12 min');
  });
});

describe('limitsText', () => {
  it('reads like a sentence a person would say', () => {
    expect(limitsText({ hits: 3, limitedMs: 7 * 3_600_000 })).toBe('Hit its limit 3 times · 7 h waiting');
    expect(limitsText({ hits: 1, limitedMs: 90 * 60_000 })).toBe('Hit its limit 1 time · 1.5 h waiting');
    expect(limitsText({ hits: 2, limitedMs: 25 * 60_000 })).toBe('Hit its limit 2 times · 25 min waiting');
  });

  it('leaves out the waiting part when no end was recorded', () => {
    expect(limitsText({ hits: 1, limitedMs: 0 })).toBe('Hit its limit 1 time');
  });

  it('says so when nothing was hit, and when a wait came over from last month', () => {
    expect(limitsText({ hits: 0, limitedMs: 0 })).toBe('No limits hit');
    expect(limitsText({ hits: 0, limitedMs: 3 * 3_600_000 })).toBe('Waiting since last month · 3 h');
  });
});

describe('formatWaiting', () => {
  it('rounds to what is worth reading', () => {
    expect(formatWaiting(10_000)).toBe('1 min');
    expect(formatWaiting(2.5 * 3_600_000)).toBe('2.5 h');
    expect(formatWaiting(11.4 * 3_600_000)).toBe('11 h');
  });
});

describe('filesText', () => {
  it('shows a dash when no turn told, and plain numbers otherwise', () => {
    expect(filesText({ knownTurns: 0, changed: 0, added: 0, deleted: 0 })).toBe('—');
    expect(filesText({ knownTurns: 3, changed: 0, added: 0, deleted: 0 })).toBe('No files changed');
    expect(filesText({ knownTurns: 3, changed: 1, added: 4, deleted: 0 })).toBe('1 file +4 −0');
    expect(filesText({ knownTurns: 3, changed: 12, added: 340, deleted: 45 })).toBe('12 files +340 −45');
  });
});

describe('mergedText and neighbour', () => {
  it('shows a dash for none', () => {
    expect(mergedText(0)).toBe('—');
    expect(mergedText(2)).toBe('2');
  });

  it('finds the month before and after in a newest-first list', () => {
    const month = (id: string) => ({ id, label: id, startMs: 0, endMs: 1 });
    const list = [month('2026-10'), month('2026-09'), month('2026-07')];
    expect(neighbour(list, '2026-09', 'older')).toBe('2026-07');
    expect(neighbour(list, '2026-09', 'newer')).toBe('2026-10');
    expect(neighbour(list, '2026-10', 'newer')).toBeNull();
    expect(neighbour(list, '2026-07', 'older')).toBeNull();
    expect(neighbour(list, '1999-01', 'older')).toBeNull();
  });
});

describe('plansApi', () => {
  it('explains itself outside the desktop app', async () => {
    await expect(plansApi.report()).rejects.toThrow('Open Puppet Master on your computer');
  });
});
