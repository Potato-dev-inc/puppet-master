import { describe, expect, it } from 'vitest';
import { createFocusMock, EXAMPLE_FOCUS_SUMMARY } from './focus-fixtures';
import type { FocusView } from '../lib/focus';

const ask = (mock: ReturnType<typeof createFocusMock>, cmd: string, args: Record<string, unknown> = {}) => {
  const answer = mock(cmd, args);
  expect(answer.handled).toBe(true);
  return answer.value as FocusView;
};

describe('the dev harness Focus mock', () => {
  it('leaves every other command to the normal mock', () => {
    expect(createFocusMock(undefined)('list_panes', {}).handled).toBe(false);
    expect(createFocusMock(undefined)('focus_unknown', {}).handled).toBe(false);
  });

  it('starts off by default and starts, extends and ends like the app', () => {
    const mock = createFocusMock(undefined);
    expect(ask(mock, 'focus_state').on).toBe(false);
    const started = ask(mock, 'focus_start', { minutes: 25 });
    expect(started.on).toBe(true);
    expect(started.minutes).toBe(25);
    const extended = ask(mock, 'focus_extend');
    expect(extended.endsAtMs! - started.endsAtMs!).toBe(15 * 60_000);
    const ended = ask(mock, 'focus_end');
    expect(ended.on).toBe(false);
    expect(ended.lastSummary?.line).toContain('While you focused');
    mock('focus_summary_dismiss', {});
    expect(ask(mock, 'focus_state').lastSummary).toBeNull();
  });

  it('a scenario can start with Focus on', () => {
    const view = ask(createFocusMock({ on: true, minutesLeft: 31, heldTotal: 3 }), 'focus_state');
    expect(view.on).toBe(true);
    expect(view.heldTotal).toBe(3);
    const left = (view.endsAtMs! - Date.now()) / 60_000;
    expect(left).toBeGreaterThan(30.9);
    expect(left).toBeLessThanOrEqual(31);
  });

  it('refuses a length outside 5 to 240 and extend while off', () => {
    const mock = createFocusMock(undefined);
    expect(() => mock('focus_start', { minutes: 3 })).toThrow(/FOCUS_BAD_MINUTES/);
    expect(() => mock('focus_extend', {})).toThrow(/FOCUS_NOT_ON/);
  });

  it('a scenario can show a just-ended summary', () => {
    const view = ask(createFocusMock({ lastSummary: EXAMPLE_FOCUS_SUMMARY }), 'focus_state');
    expect(view.on).toBe(false);
    expect(view.lastSummary).toEqual(EXAMPLE_FOCUS_SUMMARY);
  });

  it('saves settings', () => {
    const mock = createFocusMock(undefined);
    const view = ask(mock, 'focus_settings_set', { settings: { default_minutes: 90, quiet_blocking: false } });
    expect(view.settings).toEqual({ default_minutes: 90, quiet_blocking: false });
    expect(view.minutes).toBe(90);
  });
});
