// The Night Shift harness scenarios: they are registered, and the mocked `night_*` commands answer the shapes the UI reads.
import { describe, expect, it } from 'vitest';
import { BUSY_SENTENCE } from '../lib/night';
import {
  CONFLICT_STATE,
  EMPTY_STATE,
  MORNING_4_OF_5,
  MORNING_CONFLICT,
  MORNING_STATE,
  PLANS,
  RUNNING_STATE,
  nightMock,
} from './night-fixtures';
import { scenarios } from './scenarios';

const NAMES = [
  'night-tonight-empty',
  'night-tonight-filled',
  'night-running',
  'night-waiting',
  'night-morning-4-of-5',
  'night-morning-conflict',
];

describe('night scenarios', () => {
  it('are all registered under their own name, on the cockpit', () => {
    for (const name of NAMES) {
      expect(scenarios[name]?.name).toBe(name);
      expect(scenarios[name]?.route).toBe('cockpit');
    }
  });

  it('the morning page reads "4 of 5 done, tests green. 1 needs you."', () => {
    expect(MORNING_4_OF_5.line).toBe('4 of 5 done, tests green. 1 needs you.');
    expect(MORNING_4_OF_5).toMatchObject({ total: 5, done: 4, green: 4, needsYou: 1, keepAll: 4 });
  });

  it('a Keep that conflicted is shown as failed and cannot be kept again', () => {
    const row = MORNING_CONFLICT.tasks[0];
    expect(row.review).toBe('keep_failed');
    expect(row.canKeep).toBe(false);
    expect(row.canThrowAway).toBe(false);
    expect(row.reviewText).toMatch(/^This conflicts with a task you already kept \(src\/cart\.ts\)\. Throw it away/);
  });

  it('a running night blocks Start and says why', () => {
    expect(RUNNING_STATE.canStart).toBe(false);
    expect(RUNNING_STATE.startBlocked).toBe(BUSY_SENTENCE);
    expect(scenarios['night-running'].night?.state).toBe(RUNNING_STATE);
    expect(scenarios['night-morning-conflict'].night?.state).toBe(CONFLICT_STATE);
    expect(scenarios['night-morning-4-of-5'].night?.state).toBe(MORNING_STATE);
  });
});

describe('nightMock', () => {
  it('leaves other commands alone', () => {
    expect(nightMock(undefined, 'list_panes', {}).handled).toBe(false);
    expect(nightMock(undefined, 'night_nope', {}).handled).toBe(false);
  });

  it('answers the state with the scenario state, or an empty Tonight', () => {
    expect(nightMock(undefined, 'night_state', {}).value).toBe(EMPTY_STATE);
    expect(nightMock({ state: RUNNING_STATE }, 'night_state', {}).value).toBe(RUNNING_STATE);
  });

  it('answers the plans, and the checks of a project', () => {
    expect(nightMock(undefined, 'night_plans', { projects: ['C:/work/app'] }).value).toBe(PLANS);
    expect(nightMock(undefined, 'night_checks_get', { project: 'C:/work/app' }).value).toMatchObject({ baselineRed: null });
  });

  it('echoes the checks it was asked to save, split into argv', () => {
    const list = [{ name: 'lint', command: 'npm run lint', timeoutMin: 5 }];
    expect(nightMock(undefined, 'night_checks_set', { project: 'C:/work/app', list }).value).toEqual([
      { name: 'lint', argv: ['npm', 'run', 'lint'], timeoutMin: 5 },
    ]);
  });

  it('answers the run-changing commands with the current run', () => {
    const s = { state: MORNING_STATE };
    for (const cmd of ['night_stop', 'night_keep', 'night_throw_away', 'night_keep_all_ready']) {
      expect(nightMock(s, cmd, { taskId: 'nt-00000001' }).value).toBe(MORNING_4_OF_5);
    }
  });

  it('echoes the settings it was given and the report path it was asked to save', () => {
    expect(nightMock(undefined, 'night_settings_get', {}).value).toBe(EMPTY_STATE.settings);
    const settings = { ...EMPTY_STATE.settings, parallel: 2 };
    expect(nightMock(undefined, 'night_settings_set', { settings }).value).toBe(settings);
    expect(nightMock(undefined, 'night_report_save', { path: 'C:/work/night.html' }).value).toBe('C:/work/night.html');
  });
});
