// The routines client helpers: the form becomes the request or one sentence that says what is missing, the row's
// words for the next run, and the fake that the tab tests use.
import { describe, expect, it } from 'vitest';
import {
  buildRoutineInput,
  DEFAULT_ROUTINE_FORM,
  fakeRoutinesTransport,
  routineStatusLine,
  type RoutineFormValues,
  type RoutineView,
} from './potato-routines';

const form = (over: Partial<RoutineFormValues>): RoutineFormValues => ({ ...DEFAULT_ROUTINE_FORM, title: 'Sweep', prompt: 'Check the inbox.', ...over });

const row = (over: Partial<RoutineView> = {}): RoutineView => ({
  id: 'r-1',
  bot: 'nimbus',
  title: 'Sweep',
  prompt: 'Check the inbox.',
  schedule: { kind: 'hourly' },
  summary: 'Every hour',
  status: 'active',
  paused: false,
  done: false,
  lastRunMs: null,
  nextRunMs: new Date(2026, 10, 2, 9, 0).getTime(),
  ...over,
});

describe('buildRoutineInput', () => {
  it('turns each frequency into its schedule', () => {
    expect(buildRoutineInput(form({ freq: 'hourly' }))).toEqual({ title: 'Sweep', prompt: 'Check the inbox.', schedule: { kind: 'hourly' } });
    expect(buildRoutineInput(form({ freq: 'daily', time: '18:30' }))).toMatchObject({ schedule: { kind: 'daily', time: '18:30' } });
    expect(buildRoutineInput(form({ freq: 'every', minutes: 15 }))).toMatchObject({ schedule: { kind: 'everyMinutes', minutes: 15 } });
    expect(buildRoutineInput(form({ freq: 'once', at: '2026-11-05T09:30' }))).toMatchObject({ schedule: { kind: 'once', at: '2026-11-05T09:30' } });
  });

  it('says in one sentence what is missing', () => {
    expect(buildRoutineInput(form({ title: '  ' }))).toBe('Give the routine a name.');
    expect(buildRoutineInput(form({ prompt: '' }))).toBe('Write what the bot should do.');
    expect(buildRoutineInput(form({ freq: 'daily', time: '' }))).toBe('Pick a time.');
    expect(buildRoutineInput(form({ freq: 'every', minutes: 14 }))).toBe('Pick every 15 minutes or more.');
    expect(buildRoutineInput(form({ freq: 'once', at: '' }))).toBe('Pick a date and time.');
  });
});

describe('routineStatusLine', () => {
  const now = new Date(2026, 10, 2, 8, 0).getTime();

  it('says when the next run is, or that it is paused or done', () => {
    expect(routineStatusLine(row({ nextRunMs: new Date(2026, 10, 2, 9, 0).getTime() }), now)).toMatch(/^Next run today at 9:00/);
    expect(routineStatusLine(row({ nextRunMs: new Date(2026, 10, 3, 9, 0).getTime() }), now)).toMatch(/^Next run tomorrow at 9:00/);
    expect(routineStatusLine(row({ status: 'paused', paused: true, nextRunMs: null }), now)).toBe('Paused');
    expect(routineStatusLine(row({ status: 'done', done: true, nextRunMs: null }), now)).toBe('Ran once. Done.');
  });
});

describe('fakeRoutinesTransport', () => {
  it('keeps the rows it is given and records every call', async () => {
    const fake = fakeRoutinesTransport([row()]);
    expect(await fake.list('nimbus')).toHaveLength(1);
    const made = await fake.create('nimbus', { title: 'Second', prompt: 'Do it.', schedule: { kind: 'hourly' } });
    expect(made.bot).toBe('nimbus');
    await fake.remove('r-1');
    expect((await fake.list('nimbus')).map((r) => r.title)).toEqual(['Second']);
    expect(fake.calls.map((c) => c.method)).toEqual(['list', 'create', 'remove', 'list']);
  });
});
