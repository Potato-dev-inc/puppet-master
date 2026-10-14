import { describe, expect, it } from 'vitest';
import {
  DEFAULT_STANDBY_CONFIG,
  NOT_TERMINAL_NOTE,
  NO_WARM_MODE_NOTE,
  clampInt,
  formatDuration,
  idleDisplayValue,
  idleUnitFor,
  preloadCount,
  preloadProviders,
  summarizePreload,
  withIdleMinutes,
  withIdleValue,
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

const agent = (id: string, visible: boolean, standbyVisible: boolean, extra: Partial<AgentDescriptor> = {}): AgentDescriptor =>
  ({
    id,
    label: id.toUpperCase(),
    kind: 'cli',
    dispatchable: true,
    modes: { visible, headless: true },
    standby: { visible: standbyVisible, headless: false, experimental: false },
    ...extra,
  }) as unknown as AgentDescriptor;

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
      { id: 'claude', label: 'CLAUDE', supported: true, experimental: false, unsupportedNote: null },
      { id: 'codex', label: 'CODEX', supported: false, experimental: false, unsupportedNote: NO_WARM_MODE_NOTE },
    ]);
  });

  it('flags an experimental warm mode, and says why a non-terminal agent cannot be kept warm', () => {
    const rows = preloadProviders([
      agent('myagent', true, true, { standby: { visible: true, headless: false, experimental: true } } as Partial<AgentDescriptor>),
      agent('opencode_native', true, false, { kind: 'native_protocol' }),
      agent('cursor', true, false, { dispatchable: false }),
    ]);
    expect(rows.map((row) => [row.id, row.supported, row.experimental, row.unsupportedNote])).toEqual([
      ['myagent', true, true, null],
      ['opencode_native', false, false, NOT_TERMINAL_NOTE],
      ['cursor', false, false, NOT_TERMINAL_NOTE],
    ]);
  });

  it('never marks an unsupported agent experimental', () => {
    const [row] = preloadProviders([
      agent('ghost', true, false, { standby: { visible: false, headless: false, experimental: true } } as Partial<AgentDescriptor>),
    ]);
    expect(row.experimental).toBe(false);
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

describe('idle timeout in a faithful unit', () => {
  it('shows whole minutes in minutes and anything else in seconds', () => {
    expect(idleUnitFor(5)).toBe('seconds');
    expect(idleUnitFor(59)).toBe('seconds');
    expect(idleUnitFor(60)).toBe('minutes');
    expect(idleUnitFor(90)).toBe('seconds');
    expect(idleUnitFor(1200)).toBe('minutes');
  });

  it('displays the stored value exactly in the chosen unit', () => {
    expect(idleDisplayValue(5, 'seconds')).toBe(5);
    expect(idleDisplayValue(1200, 'minutes')).toBe(20);
  });

  it('saves a typed value in seconds or minutes, clamped to the backend range', () => {
    expect(withIdleValue(DEFAULT_STANDBY_CONFIG, 5, 'seconds').idle_timeout_secs).toBe(5);
    expect(withIdleValue(DEFAULT_STANDBY_CONFIG, 1, 'seconds').idle_timeout_secs).toBe(5);
    expect(withIdleValue(DEFAULT_STANDBY_CONFIG, 2, 'minutes').idle_timeout_secs).toBe(120);
    expect(withIdleValue(DEFAULT_STANDBY_CONFIG, 99999, 'minutes').idle_timeout_secs).toBe(86400);
  });

  it('keeps the saved timeout when the typed value is not a number, and never mutates the input', () => {
    const config = { ...DEFAULT_STANDBY_CONFIG, idle_timeout_secs: 1200 };
    const before = structuredClone(config);
    expect(withIdleValue(config, Number.NaN, 'seconds').idle_timeout_secs).toBe(1200);
    expect(withIdleValue(config, 30, 'seconds').idle_timeout_secs).toBe(30);
    expect(config).toEqual(before);
    expect(config.idle_timeout_secs).toBe(1200);
  });
});
