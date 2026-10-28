// Night Shift for the harness: what the mocked `night_*` commands answer. Example data only. The shapes are
// `lib/night.ts` (mirroring `night_shift/view.rs` and `commands/night.rs`); the words are the backend's, and the
// summary line is built with the same rule as `report::line` (`summaryLine`).
import {
  type NightCheckInput,
  type NightCheckView,
  type NightChecksView,
  type NightPlanView,
  type NightSettingsView,
  type NightStateView,
  type NightTaskView,
  type NightView,
  BUSY_SENTENCE,
  LABS_OFF_SENTENCE,
  summaryLine,
} from '../lib/night';

export interface NightScenario {
  /** What `night_state` answers. Default: Labs on, no run yet. */
  state?: NightStateView;
  /** What `night_plans` answers. Default: Codex ready, Claude Code out of usage. */
  plans?: NightPlanView[];
}

export const SETTINGS: NightSettingsView = {
  plans: ['codex'],
  stopHhmm: '07:00',
  parallel: 1,
  turns: 12,
  minutes: 90,
  awakeAsked: false,
};

export const PLANS: NightPlanView[] = [
  { key: 'codex', label: 'Codex', ready: true, whyNot: null, outUntil: null, triedHere: { 'C:/work/app': true } },
  {
    key: 'claude',
    label: 'Claude Code',
    ready: false,
    whyNot: 'Out of usage until 16:40.',
    outUntil: 'Out of usage until 16:40',
    triedHere: {},
  },
];

export const CHECKS: NightChecksView = {
  declared: [{ name: 'tests', argv: ['npm', 'test'], timeoutMin: 10 }],
  suggestions: [{ name: 'lint', argv: ['npm', 'run', 'lint'], source: 'package.json' }],
  warnings: [],
  baselineRed: null,
};

/** A view with nothing run yet: Tonight shows the sheet, Start is on. */
export const EMPTY_STATE: NightStateView = {
  enabled: true,
  run: null,
  canStart: true,
  startBlocked: null,
  history: [],
  settings: SETTINGS,
};

/** Labs off: Start is off and the sentence says where to turn it on. */
export const LABS_OFF_STATE: NightStateView = {
  ...EMPTY_STATE,
  enabled: false,
  canStart: false,
  startBlocked: LABS_OFF_SENTENCE,
};

const TASK: NightTaskView = {
  id: 'nt-00000001',
  title: 'Fix the cart total rounding.',
  project: 'app',
  plan: 'Codex',
  state: 'finished',
  stateText: 'Finished',
  until: null,
  class: null,
  headline: null,
  reason: null,
  tests: null,
  testsBefore: null,
  testChangeLines: [],
  files: null,
  turns: 0,
  duration: null,
  summaries: [],
  attempts: [],
  review: 'pending',
  reviewText: null,
  into: null,
  canKeep: false,
  canThrowAway: false,
  paneId: null,
};

const READY_TASK: NightTaskView = {
  ...TASK,
  class: 'ready',
  headline: 'Tests pass 42 of 42',
  tests: { passed: 42, total: 42 },
  testsBefore: { passed: 40, total: 42 },
  files: { count: 5, added: 120, removed: 34 },
  turns: 3,
  duration: '38 min',
  summaries: ['Fixed the rounding in cart.ts'],
  attempts: [{ n: 1, label: 'Codex', end: 'completed', sentence: null, started: '23:10', ended: '23:48' }],
  canKeep: true,
  canThrowAway: true,
  paneId: 'pane-42',
};

const MORNING_TASKS: NightTaskView[] = [
  { ...READY_TASK, id: 'nt-00000001', title: 'Fix the cart total rounding.' },
  {
    ...READY_TASK,
    id: 'nt-00000002',
    title: 'Rename the config loader.',
    project: 'app',
    plan: 'Claude Code',
    attempts: [{ n: 1, label: 'Claude Code', end: 'completed', sentence: null, started: '23:15', ended: '00:20' }],
    turns: 5,
    duration: '1 h 5 min',
    paneId: 'pane-43',
  },
  {
    ...READY_TASK,
    id: 'nt-00000003',
    title: 'Add the export button.',
    review: 'kept',
    reviewText: 'Kept · merged into main',
    into: 'main',
    canKeep: false,
    canThrowAway: false,
    paneId: 'pane-44',
  },
  {
    ...READY_TASK,
    id: 'nt-00000004',
    title: 'Update the account page copy.',
    turns: 4,
    duration: '45 min',
    paneId: 'pane-45',
  },
  {
    ...TASK,
    id: 'nt-00000005',
    title: 'Fix broken links in /guides',
    project: 'docs-site',
    plan: 'Codex',
    class: 'needs_look',
    headline: 'Tests fail: 9 of 12 pass',
    reason: 'Look at the failing tests before you keep it.',
    tests: { passed: 9, total: 12 },
    testsBefore: { passed: 7, total: 12 },
    testChangeLines: ['Changed tests/links.test.ts'],
    files: { count: 2, added: 14, removed: 3 },
    turns: 12,
    duration: '1 h 30 min',
    summaries: ['Fixed two of the broken links.'],
    attempts: [
      {
        n: 1,
        label: 'Codex',
        end: 'gate',
        sentence: 'Codex asked to trust the folder.',
        started: '01:20',
        ended: '02:50',
      },
    ],
    canKeep: true,
    canThrowAway: true,
    paneId: 'pane-46',
  },
];

const CONFLICT_TASK: NightTaskView = {
  ...READY_TASK,
  id: 'nt-00000006',
  title: 'Tidy the cart helpers.',
  review: 'keep_failed',
  reviewText:
    'This conflicts with a task you already kept (src/cart.ts). Throw it away, or merge it yourself from its branch ns/nt-00000006.',
  canKeep: false,
  canThrowAway: false,
};

function countsOf(tasks: NightTaskView[]) {
  const ready = tasks.filter((task) => task.class === 'ready').length;
  const noChecks = tasks.filter((task) => task.class === 'no_checks').length;
  const needsLook = tasks.filter((task) => task.class === 'needs_look').length;
  return {
    total: tasks.length,
    done: ready + noChecks,
    green: ready,
    needsYou: needsLook,
    didntRun: tasks.filter((task) => task.class === 'didnt_run').length,
    noChanges: tasks.filter((task) => task.class === 'no_changes').length,
  };
}

function viewOf(tasks: NightTaskView[], over: Partial<NightView>): NightView {
  return {
    id: 'ns-0123456789ab',
    rev: 1,
    state: 'done',
    line: summaryLine(countsOf(tasks)),
    ...countsOf(tasks),
    keepAll: tasks.filter((task) => task.canKeep).length,
    from: '23:10',
    to: '05:22',
    turns: tasks.reduce((sum, task) => sum + task.turns, 0),
    duration: '6 h 12 min',
    byPlan: [],
    tasks,
    events: [],
    ...over,
  };
}

/** Morning: 4 of 5 done, tests green, one needs you. Four tasks wait for Keep or Throw away. */
export const MORNING_4_OF_5: NightView = viewOf(MORNING_TASKS, {
  byPlan: [
    { label: 'Codex', tasks: 3 },
    { label: 'Claude Code', tasks: 2 },
  ],
  events: [
    { at: '23:10', kind: 'started', glyph: '~', text: 'Started in app', task: null },
    { at: '23:10', kind: 'task_started', glyph: '~', text: 'Fix the cart total rounding.', task: 'nt-00000001' },
    { at: '23:48', kind: 'task_done', glyph: 'v', text: 'Tests pass 42 of 42', task: 'nt-00000001' },
    { at: '02:50', kind: 'asked', glyph: '+', text: 'Codex asked to trust the folder.', task: 'nt-00000005' },
    { at: '05:22', kind: 'finished', glyph: 'v', text: 'Finished at 05:22', task: null },
  ],
});

/** Morning after a Keep that conflicted: the row says so and offers Throw away. */
export const MORNING_CONFLICT: NightView = viewOf([CONFLICT_TASK, { ...READY_TASK, id: 'nt-00000007', title: 'Add a total test.' }], {
  byPlan: [{ label: 'Codex', tasks: 2 }],
});

/** Tonight: a run has started and is working. Start is off until it is finished or stopped. */
export const RUNNING: NightView = viewOf(
  [
    { ...TASK, id: 'nt-00000001', title: 'Fix the cart total rounding.', state: 'working', stateText: 'Working', attempts: [{ n: 1, label: 'Codex', end: 'running', sentence: null, started: '23:10', ended: null }], plan: 'Codex' },
    { ...TASK, id: 'nt-00000002', title: 'Rename the config loader.', state: 'queued', stateText: 'In the queue' },
    { ...TASK, id: 'nt-00000003', title: 'Add the export button.', state: 'queued', stateText: 'In the queue' },
  ],
  { state: 'running', to: null, from: '23:10', turns: 0, duration: 'under a minute', line: '0 of 3 done' },
);

/** Tonight: a task waits for a plan with usage left. */
export const WAITING: NightView = viewOf(
  [
    { ...TASK, id: 'nt-00000001', title: 'Fix the cart total rounding.', state: 'waiting', stateText: 'Waiting: Codex is out of usage.', until: '01:10', plan: 'Codex' },
    { ...TASK, id: 'nt-00000002', title: 'Rename the config loader.', state: 'working', stateText: 'Working', plan: 'Claude Code' },
  ],
  { state: 'running', to: null, from: '23:10', turns: 2, duration: '12 min', line: '0 of 2 done' },
);

/** The Tonight sheet opened with the saved plan, stop time and project checks. The person types the tasks. */
export const TONIGHT_FILLED: NightStateView = {
  ...EMPTY_STATE,
  settings: { ...SETTINGS, parallel: 2, awakeAsked: true },
};

/** A run is working: Start is off, and the sentence says why. */
export const RUNNING_STATE: NightStateView = {
  ...EMPTY_STATE,
  run: RUNNING,
  canStart: false,
  startBlocked: BUSY_SENTENCE,
};

/** A run waits for a plan with usage left. */
export const WAITING_STATE: NightStateView = { ...RUNNING_STATE, run: WAITING };

/** The morning after: 4 of 5 done, with the history of earlier nights. */
export const MORNING_STATE: NightStateView = {
  ...EMPTY_STATE,
  run: MORNING_4_OF_5,
  history: [{ id: 'ns-aaaaaaaaaaaa', line: '2 of 2 done, tests green.' }],
};

/** The morning after with a Keep that conflicted. */
export const CONFLICT_STATE: NightStateView = { ...MORNING_STATE, run: MORNING_CONFLICT };

export function nightMock(s: NightScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('night_')) return { handled: false };
  const state = s?.state ?? EMPTY_STATE;
  const run = state.run;
  switch (cmd) {
    case 'night_state':
      return { handled: true, value: state };
    case 'night_plans':
      return { handled: true, value: s?.plans ?? PLANS };
    case 'night_checks_get':
      return { handled: true, value: CHECKS };
    case 'night_checks_set': {
      const list = (args.list as NightCheckInput[] | undefined) ?? [];
      const saved: NightCheckView[] = list.map((check) => ({ name: check.name, argv: check.command.split(' '), timeoutMin: check.timeoutMin }));
      return { handled: true, value: saved };
    }
    case 'night_start':
      return { handled: true, value: { ...MORNING_4_OF_5, state: 'scheduled', tasks: [], events: [], line: '0 of 0 done' } };
    case 'night_stop':
    case 'night_keep':
    case 'night_throw_away':
    case 'night_keep_all_ready':
      return { handled: true, value: run };
    case 'night_seen':
      return { handled: true, value: null };
    case 'night_report_html':
      return { handled: true, value: '<!doctype html><title>Night Shift</title>' };
    case 'night_report_save':
      return { handled: true, value: args.path };
    case 'night_settings_get':
      return { handled: true, value: state.settings };
    case 'night_settings_set':
      return { handled: true, value: args.settings };
    default:
      return { handled: false };
  }
}

