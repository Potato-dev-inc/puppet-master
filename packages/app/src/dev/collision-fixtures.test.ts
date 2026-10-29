// The Collision Alert harness scenarios: they are registered, and the mocked `collision_*` commands answer the shapes the UI reads.
import { describe, expect, it } from 'vitest';
import { isLoud } from '../lib/collision';
import { ALERTS, CLASH, QUIET, QUIET_ALERTS, collisionMock } from './collision-fixtures';
import { PANE_CLAUDE, PANE_CODEX } from './fixtures';
import { scenarios } from './scenarios';

const NAMES = ['collision-card', 'collision-card-light', 'collision-quiet', 'collision-banner', 'collision-settings'];

describe('collision scenarios', () => {
  it('are all registered under their own name', () => {
    for (const name of NAMES) expect(scenarios[name]?.name).toBe(name);
  });

  it('bring the alerts they show, and the banner selects the pane it is above', () => {
    expect(scenarios['collision-card'].collision?.alerts).toBe(ALERTS);
    expect(scenarios['collision-card-light'].collision?.alerts).toBe(ALERTS);
    expect(scenarios['collision-card-light'].theme).toBe('light');
    expect(scenarios['collision-quiet'].collision?.alerts).toBe(QUIET_ALERTS);
    expect(scenarios['collision-banner'].collision?.alerts).toBe(ALERTS);
    expect(scenarios['collision-banner'].select).toBe(PANE_CLAUDE);
  });
});

describe('the example alerts', () => {
  it('name the two example panes, and only the clash is loud', () => {
    for (const view of [CLASH, QUIET]) expect(view.sides.map((s) => s.paneId)).toEqual([PANE_CLAUDE, PANE_CODEX]);
    expect(isLoud(CLASH)).toBe(true);
    expect(isLoud(QUIET)).toBe(false);
    expect(QUIET.advice).toBeNull();
    expect(CLASH.id).toMatch(/^ca-[0-9a-f]{12}$/);
    expect(QUIET.id).toMatch(/^ca-[0-9a-f]{12}$/);
  });
});

describe('collisionMock', () => {
  it('leaves other commands alone', () => {
    expect(collisionMock(undefined, 'list_panes', {}).handled).toBe(false);
    expect(collisionMock(undefined, 'collision_nope', {}).handled).toBe(false);
  });

  it('lists nothing by default and the scenario alerts when it has some', () => {
    expect(collisionMock(undefined, 'collision_list', {}).value).toEqual([]);
    expect(collisionMock({ alerts: ALERTS }, 'collision_list', {}).value).toEqual(ALERTS);
  });

  it('hides an ignored alert for the rest of the page, per scenario', () => {
    const s = { alerts: [CLASH, QUIET] };
    expect(collisionMock(s, 'collision_ignore', { id: CLASH.id }).handled).toBe(true);
    expect(collisionMock(s, 'collision_list', {}).value).toEqual([QUIET]);
    expect(collisionMock({ alerts: ALERTS }, 'collision_list', {}).value).toEqual(ALERTS);
  });

  it('answers the switch: on by default, saved as given', () => {
    expect(collisionMock(undefined, 'collision_settings_get', {}).value).toEqual({ enabled: true });
    expect(collisionMock({ enabled: false }, 'collision_settings_get', {}).value).toEqual({ enabled: false });
    expect(collisionMock(undefined, 'collision_settings_set', { settings: { enabled: false } }).value).toEqual({ enabled: false });
  });
});
