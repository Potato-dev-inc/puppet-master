import { describe, expect, it } from 'vitest';
import {
  BUSY_SENTENCE,
  LABS_OFF_SENTENCE,
  type NightPlanView,
  type NightStartInput,
  type StartCheck,
  startBlockedReason,
  summaryLine,
} from './night';

// Local noon on 28 Oct 2026. A start with no time means this moment.
const NOON = new Date(2026, 9, 28, 12, 0, 0, 0).getTime();

const PLANS: NightPlanView[] = [
  { key: 'codex', label: 'Codex', ready: true, whyNot: null, outUntil: null, triedHere: {} },
  { key: 'claude', label: 'Claude Code', ready: false, whyNot: 'Out of usage until 16:40.', outUntil: null, triedHere: {} },
];

const TASK = { project: 'C:/work/app', text: 'Fix the cart.' };

function input(over: Partial<NightStartInput> = {}): NightStartInput {
  return { tasks: [TASK], plans: ['codex'], stopHhmm: '07:00', startHhmm: '23:10', parallel: 1, keepAwake: false, ...over };
}

function check(over: Partial<StartCheck> = {}, inputOver: Partial<NightStartInput> = {}): StartCheck {
  return { enabled: true, busy: false, plans: PLANS, awakeAsked: false, input: input(inputOver), nowMs: NOON, ...over };
}

const UNDO = 'Missions need Undo to go back when a turn makes things worse. Undo is off for this folder.';
const BUDGET = 'Task 1: turns must be 3 to 40, and minutes 20 to 240.';

// [what is wrong, the draft, the code and sentence the sheet shows (null = Start can run)]
const MATRIX: Array<[string, StartCheck, [string, string] | null]> = [
  ['Labs is off', check({ enabled: false }), ['NIGHT_SHIFT_OFF', LABS_OFF_SENTENCE]],
  ['an unfinished run exists', check({ busy: true }), ['RUN_ACTIVE', BUSY_SENTENCE]],
  ['Undo is off', check({ undoOn: false }), ['MISSION_UNDO_OFF', UNDO]],
  ['no task is added', check({}, { tasks: [] }), ['NO_TASKS', 'Add at least one task.']],
  [
    'eleven tasks',
    check({}, { tasks: Array.from({ length: 11 }, () => TASK) }),
    ['TOO_MANY_TASKS', 'Night Shift takes up to 10 tasks.'],
  ],
  [
    'a plan is not on this computer',
    check({}, { plans: ['gemini'] }),
    ['PLAN_UNKNOWN', 'That plan is not on this computer. Pick it again under Plans.'],
  ],
  ['a ticked plan is out of usage', check({}, { plans: ['claude'] }), ['PLAN_NOT_READY', 'Out of usage until 16:40.']],
  ['no plan is ticked', check({}, { plans: [] }), ['NO_PLAN', 'Tick at least one plan to use.']],
  ['the start time is not a time', check({}, { startHhmm: '25:00' }), ['BAD_TIME', 'Pick a time like 23:30.']],
  ['the stop time is not a time', check({}, { stopHhmm: '7:00' }), ['BAD_TIME', 'Pick a time like 07:00.']],
  [
    'the stop is 20 minutes after the start',
    check({}, { startHhmm: '23:10', stopHhmm: '23:30' }),
    ['STOP_TOO_SOON', 'Pick a stop time at least 30 minutes after the start.'],
  ],
  [
    'the stop is 18 hours after a start now',
    check({}, { startHhmm: null, stopHhmm: '06:00' }),
    ['STOP_TOO_LATE', 'Pick a stop time within 14 hours of the start.'],
  ],
  [
    'keep awake is asked but not confirmed',
    check({}, { keepAwake: true }),
    ['AWAKE_NOT_CONFIRMED', 'Confirm that the computer may stay awake first.'],
  ],
  ['a task has no project', check({}, { tasks: [{ project: '  ', text: 'x' }] }), ['NO_PROJECT', 'Pick a project for task 1.']],
  [
    'a task folder is not git',
    check({ isGit: () => false }),
    [
      'MISSION_NOT_GIT',
      'Task 1: this project is not a git project with a commit. Night Shift works in a separate copy of it.',
    ],
  ],
  [
    'a task has no words',
    check({}, { tasks: [{ project: 'C:/work/app', text: '   ' }] }),
    ['TASK_TEXT_EMPTY', 'Task 1 needs words: what should it do?'],
  ],
  [
    'a task is longer than 2000 characters',
    check({}, { tasks: [{ project: 'C:/work/app', text: 'x'.repeat(2001) }] }),
    ['TASK_TEXT_TOO_LONG', 'Task 1 is longer than 2000 characters.'],
  ],
  ['turns below 3', check({}, { tasks: [{ ...TASK, turns: 2 }] }), ['BUDGET_RANGE', BUDGET]],
  ['minutes above 240', check({}, { tasks: [{ ...TASK, minutes: 241 }] }), ['BUDGET_RANGE', BUDGET]],
  [
    'a task uses a plan that is not ticked',
    check({}, { tasks: [{ ...TASK, who: 'claude' }] }),
    ['AGENT_NOT_TICKED', 'Task 1 uses a plan that is not ticked under Plans.'],
  ],
  [
    'the second task is the one with no project',
    check({}, { tasks: [TASK, { project: '', text: 'x' }] }),
    ['NO_PROJECT', 'Pick a project for task 2.'],
  ],
  ['a clean draft can start', check(), null],
  ['who "any" and an empty who both pass', check({}, { tasks: [{ ...TASK, who: 'any' }, { ...TASK, who: '' }] }), null],
  ['the same plan ticked twice is one plan', check({}, { plans: ['codex', 'codex'] }), null],
  ['keep awake confirmed passes', check({ awakeAsked: true }, { keepAwake: true }), null],
];

describe('startBlockedReason', () => {
  it.each(MATRIX)('%s', (_name, draft, expected) => {
    const got = startBlockedReason(draft);
    if (expected === null) expect(got).toBeNull();
    else expect(got).toEqual({ code: expected[0], message: expected[1] });
  });

  it('gives the first reason in order, so Labs off wins over a busy run', () => {
    expect(startBlockedReason(check({ enabled: false, busy: true }))?.code).toBe('NIGHT_SHIFT_OFF');
  });

  it('checks each task in order and names the first one that fails', () => {
    const got = startBlockedReason(check({}, { tasks: [TASK, { project: 'C:/work/app', text: '' }] }));
    expect(got?.message).toBe('Task 2 needs words: what should it do?');
  });
});

describe('summaryLine', () => {
  // The same cases as `report_tests.rs`, so the desktop and the Rust side say the same words.
  it.each([
    [{ total: 5, done: 5, green: 5, needsYou: 0, didntRun: 0, noChanges: 0 }, '5 of 5 done, tests green'],
    [{ total: 5, done: 4, green: 4, needsYou: 1, didntRun: 0, noChanges: 0 }, '4 of 5 done, tests green. 1 needs you.'],
    [{ total: 5, done: 0, green: 0, needsYou: 3, didntRun: 2, noChanges: 0 }, "0 of 5 done. 3 need you. 2 didn't run."],
    [{ total: 1, done: 0, green: 0, needsYou: 1, didntRun: 0, noChanges: 0 }, '0 of 1 done. 1 needs you.'],
    [{ total: 2, done: 2, green: 0, needsYou: 0, didntRun: 0, noChanges: 0 }, '2 of 2 done, no tests ran'],
    [{ total: 4, done: 3, green: 2, needsYou: 1, didntRun: 0, noChanges: 0 }, '3 of 4 done, tests green on 2. 1 needs you.'],
    [{ total: 2, done: 1, green: 1, needsYou: 0, didntRun: 0, noChanges: 1 }, '1 of 2 done, tests green. 1 changed nothing.'],
    [
      { total: 3, done: 1, green: 1, needsYou: 0, didntRun: 1, noChanges: 1 },
      "1 of 3 done, tests green. 1 didn't run. 1 changed nothing.",
    ],
    [{ total: 2, done: 1, green: 0, needsYou: 0, didntRun: 1, noChanges: 0 }, "1 of 2 done, no tests ran. 1 didn't run."],
    [{ total: 0, done: 0, green: 0, needsYou: 0, didntRun: 0, noChanges: 0 }, '0 of 0 done'],
  ])('summary line case %#', (counts, text) => {
    expect(summaryLine(counts)).toBe(text);
  });
});
