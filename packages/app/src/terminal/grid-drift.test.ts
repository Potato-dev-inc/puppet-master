import { describe, expect, it } from 'vitest';
import { DRIFT_CONFIRM_MS, DRIFT_COOLDOWN_MS, DRIFT_MAX_ATTEMPTS, DriftMonitor, hasGridDrift } from './grid-drift';

const g = (cols: number, rows: number) => ({ cols, rows });

describe('hasGridDrift', () => {
  it('is false when the grids match', () => expect(hasGridDrift(g(100, 30), g(100, 30), false)).toBe(false));
  it('is true when columns or rows differ', () => {
    expect(hasGridDrift(g(100, 30), g(80, 30), false)).toBe(true);
    expect(hasGridDrift(g(100, 30), g(100, 29), false)).toBe(true);
  });
  it('is false while a phone holds the PTY (the viewer follows it on purpose)', () => {
    expect(hasGridDrift(g(100, 30), g(80, 24), true)).toBe(false);
  });
  it('is false when the PTY grid is unknown or empty', () => {
    expect(hasGridDrift(g(100, 30), null, false)).toBe(false);
    expect(hasGridDrift(g(100, 30), undefined, false)).toBe(false);
    expect(hasGridDrift(g(100, 30), g(0, 0), false)).toBe(false);
    expect(hasGridDrift(g(0, 0), g(80, 24), false)).toBe(false);
  });
});

describe('DriftMonitor', () => {
  it('asks to look again first, then corrects once the drift persisted', () => {
    const m = new DriftMonitor();
    expect(m.check(g(100, 30), g(80, 24), false, 0)).toBe('confirm');
    expect(m.check(g(100, 30), g(80, 24), false, DRIFT_CONFIRM_MS - 1)).toBe('confirm');
    expect(m.check(g(100, 30), g(80, 24), false, DRIFT_CONFIRM_MS)).toBe('correct');
  });

  it('drops a drift that went away by itself (the pane list caught up)', () => {
    const m = new DriftMonitor();
    expect(m.check(g(100, 30), g(80, 24), false, 0)).toBe('confirm');
    expect(m.check(g(100, 30), g(100, 30), false, 300)).toBe('none');
    // A new drift starts its confirmation again.
    expect(m.check(g(100, 30), g(80, 24), false, 400)).toBe('confirm');
    expect(m.check(g(100, 30), g(80, 24), false, 500)).toBe('confirm');
  });

  it('restarts the confirmation when the grids change again', () => {
    const m = new DriftMonitor();
    expect(m.check(g(100, 30), g(80, 24), false, 0)).toBe('confirm');
    expect(m.check(g(90, 30), g(80, 24), false, DRIFT_CONFIRM_MS + 10)).toBe('confirm');
  });

  it('corrects at most DRIFT_MAX_ATTEMPTS times for the same pair (the app ignored a non-owner)', () => {
    const m = new DriftMonitor();
    let t = 0;
    m.check(g(100, 30), g(80, 24), false, t);
    const verdicts: string[] = [];
    for (let i = 0; i < 6; i++) {
      t += DRIFT_COOLDOWN_MS + DRIFT_CONFIRM_MS;
      verdicts.push(m.check(g(100, 30), g(80, 24), false, t));
    }
    expect(verdicts.filter((v) => v === 'correct')).toHaveLength(DRIFT_MAX_ATTEMPTS);
  });

  it('waits out the cooldown after a correction', () => {
    const m = new DriftMonitor();
    m.check(g(100, 30), g(80, 24), false, 0);
    expect(m.check(g(100, 30), g(80, 24), false, DRIFT_CONFIRM_MS)).toBe('correct');
    expect(m.check(g(100, 30), g(80, 24), false, DRIFT_CONFIRM_MS + 100)).toBe('none');
  });

  it('never acts while following a phone', () => {
    const m = new DriftMonitor();
    expect(m.check(g(100, 30), g(80, 24), true, 0)).toBe('none');
    expect(m.check(g(100, 30), g(80, 24), true, 10_000)).toBe('none');
  });
});
