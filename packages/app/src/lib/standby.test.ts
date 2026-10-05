import { describe, expect, it } from 'vitest';
import {
  DEFAULT_STANDBY_CONFIG,
  clampInt,
  formatDuration,
  isBackendEnabled,
  summarizeStandbys,
  withBackendToggle,
} from './standby';
import type { StandbyStatus } from './tauri-types';

const status = (over: Partial<StandbyStatus>): StandbyStatus => ({
  enabled: true,
  config: DEFAULT_STANDBY_CONFIG,
  counts: { total: 0, ready: 0, starting: 0 },
  standbys: [],
  blocked: [],
  recent: [],
  stats: { spawned: 0, spawn_failed: 0, claimed: 0, missed: 0, discarded: 0, expired: 0, abandoned: 0 },
  supported: [],
  ...over,
});

describe('standby settings helpers', () => {
  it('is off by default with nothing enabled per backend', () => {
    expect(DEFAULT_STANDBY_CONFIG.enabled).toBe(false);
    expect(isBackendEnabled(DEFAULT_STANDBY_CONFIG, 'claude', 'visible')).toBe(false);
  });

  it('toggles one backend and kind without mutating or touching the others', () => {
    const on = withBackendToggle(DEFAULT_STANDBY_CONFIG, 'claude', 'visible', true);
    expect(isBackendEnabled(on, 'claude', 'visible')).toBe(true);
    expect(isBackendEnabled(on, 'claude', 'headless')).toBe(false);
    expect(isBackendEnabled(on, 'cursor_agent', 'visible')).toBe(false);
    expect(DEFAULT_STANDBY_CONFIG.backends).toEqual({});
    const both = withBackendToggle(on, 'claude', 'headless', true);
    expect(both.backends.claude).toEqual({ visible: true, headless: true });
    expect(withBackendToggle(both, 'claude', 'visible', false).backends.claude).toEqual({ visible: false, headless: true });
  });

  it('clamps edited numbers and falls back on garbage', () => {
    expect(clampInt(99, 0, 8, 3)).toBe(8);
    expect(clampInt(-4, 1, 5, 2)).toBe(1);
    expect(clampInt(2.6, 1, 5, 2)).toBe(3);
    expect(clampInt(Number.NaN, 1, 5, 2)).toBe(2);
  });

  it('formats durations', () => {
    expect(formatDuration(null)).toBe('-');
    expect(formatDuration(4200)).toBe('4s');
    expect(formatDuration(125_000)).toBe('2m 5s');
    expect(formatDuration(3_900_000)).toBe('1h 5m');
  });

  it('summarizes the pool in one honest line', () => {
    expect(summarizeStandbys(null)).toMatch(/unavailable/);
    expect(summarizeStandbys(status({ enabled: false }))).toMatch(/^Off/);
    expect(summarizeStandbys(status({}))).toMatch(/no standby workers yet/);
    expect(summarizeStandbys(status({ counts: { total: 2, ready: 1, starting: 1 } }))).toBe('2 standby (1 ready, 1 starting)');
  });
});
