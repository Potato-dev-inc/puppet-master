import { describe, expect, it } from 'vitest';
import {
  DEFAULT_STANDBY_CONFIG,
  clampInt,
  formatDuration,
  preloadCount,
  preloadProviders,
  summarizePreload,
  withIdleMinutes,
  withPreloadCount,
  withPreloadEnabled,
} from './standby';
import type { AgentDescriptor } from './agentRegistry';
import type { StandbyStatus } from './tauri-types';

const status = (over: Partial<StandbyStatus>): StandbyStatus => ({
  enabled: true,
  config: { ...DEFAULT_STANDBY_CONFIG, preload: { enabled: true, providers: { claude: 1 } } },
  counts: { total: 0, ready: 0, starting: 0 },
  standbys: [],
  blocked: [],
  recent: [],
  stats: { spawned: 0, spawn_failed: 0, claimed: 0, missed: 0, discarded: 0, expired: 0, abandoned: 0 },
  supported: [],
  ...over,
});

const agent = (id: string, visible: boolean, standbyVisible: boolean): AgentDescriptor =>
  ({ id, label: id.toUpperCase(), modes: { visible, headless: true }, standby: { visible: standbyVisible, headless: false } }) as unknown as AgentDescriptor;

describe('preload settings helpers', () => {
  it('is off by default with no provider ticked', () => {
    expect(DEFAULT_STANDBY_CONFIG.preload.enabled).toBe(false);
    expect(preloadCount(DEFAULT_STANDBY_CONFIG, 'claude')).toBe(0);
  });

  it('sets one provider count without mutating or touching the others; 0 unticks', () => {
    const on = withPreloadCount(DEFAULT_STANDBY_CONFIG, 'claude', 1);
    expect(preloadCount(on, 'claude')).toBe(1);
    expect(preloadCount(on, 'kilo')).toBe(0);
    expect(DEFAULT_STANDBY_CONFIG.preload.providers).toEqual({});
    expect(withPreloadCount(on, 'claude', 2).preload.providers).toEqual({ claude: 2 });
    expect(withPreloadCount(on, 'claude', 9).preload.providers).toEqual({ claude: 2 });
    expect(withPreloadCount(on, 'claude', 0).preload.providers).toEqual({});
  });

  it('toggles the master switch and converts idle minutes to seconds', () => {
    expect(withPreloadEnabled(DEFAULT_STANDBY_CONFIG, true).preload.enabled).toBe(true);
    expect(withIdleMinutes(DEFAULT_STANDBY_CONFIG, 30).idle_timeout_secs).toBe(1800);
    expect(withIdleMinutes(DEFAULT_STANDBY_CONFIG, 0).idle_timeout_secs).toBe(60);
    expect(withIdleMinutes(DEFAULT_STANDBY_CONFIG, Number.NaN).idle_timeout_secs).toBe(1200);
  });

  it('lists the agents that can open a visible pane and marks those without a warm mode', () => {
    const rows = preloadProviders([agent('claude', true, true), agent('codex', true, false), agent('kiro', false, false)]);
    expect(rows).toEqual([
      { id: 'claude', label: 'CLAUDE', supported: true },
      { id: 'codex', label: 'CODEX', supported: false },
    ]);
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

  it('summarizes preload in one honest line', () => {
    expect(summarizePreload(null)).toMatch(/unavailable/);
    expect(summarizePreload(status({ config: DEFAULT_STANDBY_CONFIG }))).toMatch(/^Off/);
    expect(
      summarizePreload(status({ config: { ...DEFAULT_STANDBY_CONFIG, preload: { enabled: true, providers: {} } } })),
    ).toMatch(/no provider is ticked/);
    expect(summarizePreload(status({}))).toMatch(/start in a few seconds/);
    expect(summarizePreload(status({ counts: { total: 2, ready: 1, starting: 1 } }))).toBe('2 warm (1 ready, 1 starting)');
  });
});
