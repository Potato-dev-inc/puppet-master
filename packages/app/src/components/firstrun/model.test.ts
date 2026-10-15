// The welcome's step logic: skip, resume, Simple versus normal, shown once.
import { describe, expect, it } from 'vitest';
import {
  canContinue,
  finishPatch,
  initialFlow,
  isSimpleLook,
  lookFromSettings,
  lookPatch,
  reduce,
  reopenPatch,
  resumeStep,
  shouldShowWelcome,
  STEPS,
  type Action,
  type Flow,
  type ProjectPick,
} from './model';

const repo: ProjectPick = { path: 'D:/code/shop', name: 'shop', state: 'repo' };
const claude = { id: 'claude', label: 'Claude Code' };
const run = (flow: Flow, ...actions: Action[]): Flow => actions.reduce(reduce, flow);

describe('steps', () => {
  it('are the seven steps of the design, in order', () => {
    expect(STEPS).toEqual(['welcome', 'agents', 'project', 'task', 'phone', 'pack', 'done']);
  });

  it('Continue needs what each step is for', () => {
    expect(canContinue(initialFlow())).toBe(false); // nobody chose how they work
    expect(canContinue(initialFlow({ look: 'code' }))).toBe(true);
    expect(canContinue(initialFlow({ step: 'agents' }))).toBe(false); // no agent is ready
    expect(canContinue(initialFlow({ step: 'agents', ready: [claude] }))).toBe(true);
    expect(canContinue(initialFlow({ step: 'project' }))).toBe(false);
    expect(canContinue(initialFlow({ step: 'project', project: { ...repo, state: 'folder' } }))).toBe(false); // needs Git first
    expect(canContinue(initialFlow({ step: 'project', project: repo }))).toBe(true);
    expect(canContinue(initialFlow({ step: 'task' }))).toBe(false);
    expect(canContinue(initialFlow({ step: 'task', task: 'running' }))).toBe(false);
    expect(canContinue(initialFlow({ step: 'task', task: 'done' }))).toBe(true);
    for (const step of ['phone', 'pack', 'done'] as const) expect(canContinue(initialFlow({ step }))).toBe(true);
  });

  it('next does nothing while the step is not satisfied', () => {
    const flow = initialFlow({ step: 'agents' });
    expect(run(flow, { type: 'next' })).toBe(flow);
  });

  it('the whole way through, then the last button finishes', () => {
    let flow = run(initialFlow(), { type: 'look', look: 'code' }, { type: 'next' });
    expect(flow.step).toBe('agents');
    flow = run(flow, { type: 'agents', ready: [claude] }, { type: 'next' });
    expect(flow.step).toBe('project');
    flow = run(flow, { type: 'project', project: repo }, { type: 'next' });
    expect(flow.step).toBe('task');
    flow = run(flow, { type: 'task', task: 'done' }, { type: 'next' }, { type: 'next' }, { type: 'next' });
    expect(flow.step).toBe('done');
    expect(flow.finished).toBe(false);
    expect(run(flow, { type: 'next' }).finished).toBe(true);
  });
});

describe('skip', () => {
  it('Skip for now goes on from every step without what the step gives', () => {
    for (const step of STEPS.slice(0, -1)) {
      const next = run(initialFlow({ step }), { type: 'skip' });
      expect(next.step).toBe(STEPS[STEPS.indexOf(step) + 1]);
      expect(next.finished).toBe(false);
    }
  });

  it('skipping the task marks it skipped, but keeps a finished one', () => {
    expect(run(initialFlow({ step: 'task' }), { type: 'skip' }).task).toBe('skipped');
    expect(run(initialFlow({ step: 'task', task: 'done' }), { type: 'skip' }).task).toBe('done');
  });

  it('Skip setup ends the welcome from any step', () => {
    for (const step of STEPS) expect(run(initialFlow({ step }), { type: 'skipAll' }).finished).toBe(true);
  });

  it('Back never leaves the first step', () => {
    expect(run(initialFlow(), { type: 'back' }).step).toBe('welcome');
    expect(run(initialFlow({ step: 'project' }), { type: 'back' }).step).toBe('agents');
  });

  it('a different folder starts the first task over; the same one keeps it', () => {
    const base = initialFlow({ step: 'project', project: repo, task: 'done' });
    expect(run(base, { type: 'project', project: { ...repo } }).task).toBe('done');
    expect(run(base, { type: 'project', project: { ...repo, path: 'D:/code/other' } }).task).toBe('idle');
  });

  it('the same agents reported again are the same state (no extra render)', () => {
    const flow = initialFlow({ step: 'agents', ready: [claude] });
    expect(run(flow, { type: 'agents', ready: [{ ...claude }] })).toBe(flow);
  });
});

describe('resume', () => {
  it('lands on the saved step', () => {
    expect(resumeStep('agents', false)).toBe('agents');
    expect(resumeStep('phone', false)).toBe('phone');
  });
  it('a saved task step needs a folder; without one it lands on the folder step', () => {
    expect(resumeStep('task', true)).toBe('task');
    expect(resumeStep('task', false)).toBe('project');
  });
  it('an unknown or missing step, and a reopen, start at the top', () => {
    expect(resumeStep(undefined, false)).toBe('welcome');
    expect(resumeStep(null, true)).toBe('welcome');
    expect(resumeStep('bogus', true)).toBe('welcome');
    expect(resumeStep('agents', true, true)).toBe('welcome');
  });
});

describe('Simple or normal', () => {
  it('"I write code" is the normal cockpit; the other two answers are Simple mode', () => {
    expect(isSimpleLook('code')).toBe(false);
    expect(isSimpleLook('ai')).toBe(true);
    expect(isSimpleLook('none')).toBe(true);
    expect(isSimpleLook(null)).toBe(false);
    expect(lookPatch('code')).toEqual({ simple_mode: false });
    expect(lookPatch('ai')).toEqual({ simple_mode: true });
    expect(lookPatch('none')).toEqual({ simple_mode: true });
  });
  it('a reopen pre-selects what the settings already say', () => {
    expect(lookFromSettings(undefined)).toBeNull();
    expect(lookFromSettings(false)).toBe('code');
    expect(lookFromSettings(true)).toBe('ai');
  });
});

describe('shown once', () => {
  const auto = { show: true, auto: true, step: null };
  it('an automatic show happens on a fresh install with no panes', () => {
    expect(shouldShowWelcome(auto, false, 0)).toBe(true);
  });
  it('an automatic show is vetoed for someone who has used this install', () => {
    expect(shouldShowWelcome(auto, true, 0)).toBe(false);
    expect(shouldShowWelcome(auto, false, 2)).toBe(false);
  });
  it('a requested show is never vetoed', () => {
    expect(shouldShowWelcome({ show: true, auto: false, step: 'welcome' }, true, 3)).toBe(true);
  });
  it('done, or no answer from the backend, never shows', () => {
    expect(shouldShowWelcome({ show: false, auto: false, step: null }, false, 0)).toBe(false);
    expect(shouldShowWelcome(null, false, 0)).toBe(false);
  });
  it('finishing writes done and clears what a resume needed; reopening writes false', () => {
    expect(finishPatch()).toEqual({ onboarding_done: true, onboarding_step: null, onboarding_project: null, onboarding_agent: null });
    expect(reopenPatch()).toEqual({ onboarding_done: false, onboarding_step: 'welcome' });
  });
});
