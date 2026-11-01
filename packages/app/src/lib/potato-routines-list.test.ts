// The bot list's second line (UX-8): each line variant, the time words for today, tomorrow and a weekday, and the order.
import { describe, expect, it } from 'vitest';
import { listNextLine, nextRoutine, nextRunShort, type RoutineView } from './potato-routines';

// Sunday 1 Nov 2026, 15:00 (local clock).
const NOW = new Date(2026, 10, 1, 15, 0).getTime();
const at = (d: number, h: number, m = 0) => new Date(2026, 10, d, h, m).getTime();

const routine = (over: Partial<RoutineView>): RoutineView => ({
  id: 'r1',
  bot: 'fish',
  title: 'Morning sweep',
  prompt: 'Sweep the inbox',
  schedule: { kind: 'daily', time: '09:00' },
  summary: 'Every day at 9:00',
  status: 'active',
  paused: false,
  done: false,
  lastRunMs: null,
  nextRunMs: at(2, 9),
  ...over,
});

describe('nextRunShort', () => {
  it('shows only the clock for today', () => {
    expect(nextRunShort(at(1, 17, 30), NOW, 'en-US')).toMatch(/^5:30 PM$/);
  });
  it('says Tomorrow for the next day', () => {
    expect(nextRunShort(at(2, 9), NOW, 'en-US')).toMatch(/^Tomorrow 9:00 AM$/);
  });
  it('shows the weekday for later days', () => {
    expect(nextRunShort(at(4, 9), NOW, 'en-US')).toMatch(/^Wed 9:00 AM$/);
  });
  it('follows the locale when one is given', () => {
    expect(nextRunShort(at(4, 9), NOW, 'de-DE')).toMatch(/^Mi.? 0?9:00$/);
  });
});

describe('nextRoutine', () => {
  it('picks the active routine that runs first', () => {
    const later = routine({ id: 'b', title: 'Later', nextRunMs: at(4, 9) });
    const sooner = routine({ id: 'a', title: 'Sooner', nextRunMs: at(2, 6) });
    expect(nextRoutine([later, sooner])).toEqual({ title: 'Sooner', at: at(2, 6) });
  });
  it('skips paused, finished and unscheduled routines', () => {
    const rows = [
      routine({ id: 'p', status: 'paused', nextRunMs: at(1, 16) }),
      routine({ id: 'd', status: 'done', nextRunMs: at(1, 16) }),
      routine({ id: 'n', nextRunMs: null }),
    ];
    expect(nextRoutine(rows)).toBeNull();
  });
});

describe('listNextLine', () => {
  const withNext = [routine({})];

  it('waiting on the person: the accent line', () => {
    expect(listNextLine('needs', withNext, NOW)).toEqual({ tone: 'needs', text: 'Waiting for you' });
  });
  it('working: Thinking', () => {
    expect(listNextLine('working', withNext, NOW)).toEqual({ tone: 'working', text: 'Thinking…' });
  });
  it('paused: Paused, even with a routine', () => {
    expect(listNextLine('paused', withNext, NOW)).toEqual({ tone: 'quiet', text: 'Paused' });
  });
  it('idle with a routine: the next run', () => {
    expect(listNextLine(null, withNext, NOW, 'en-US')).toEqual({
      tone: 'quiet',
      text: expect.stringMatching(/^Next: Morning sweep · Tomorrow 9:00 AM$/),
    });
  });
  it('idle without a routine: no line, the preview stays', () => {
    expect(listNextLine(null, [], NOW)).toBeNull();
  });
  it('an error gets no line here (its pill shows)', () => {
    expect(listNextLine('error', withNext, NOW)).toBeNull();
  });
  it('priority: waiting beats working, working beats paused, paused beats the routine', () => {
    expect(listNextLine('needs', withNext, NOW)?.text).toBe('Waiting for you');
    expect(listNextLine('working', withNext, NOW)?.text).toBe('Thinking…');
    expect(listNextLine('paused', withNext, NOW)?.text).toBe('Paused');
  });
});
