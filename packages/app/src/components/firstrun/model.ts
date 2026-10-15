// The first-run welcome as plain data and pure functions: which step comes next, what lets a person
// continue, what a skip does, how a restart resumes, and which settings each choice writes. The
// components only draw this; the tests (model.test.ts) pin it.
//
// Settings keys (Rust reads onboarding_done in `first_run::state`):
//   onboarding_done     absent = never decided, true = finished or skipped, false = show it again
//   onboarding_step     the step the person was on
//   onboarding_project  the folder chosen for the first task
//   onboarding_agent    the agent that was ready, so a restart can still offer the first task
//   simple_mode         written the moment a look is chosen; the cockpit keeps its normal face until the
//                       welcome closes (CockpitApp), so sign-in and install windows stay visible.

export const STEPS = ['welcome', 'agents', 'project', 'task', 'phone', 'pack', 'done'] as const;
export type StepId = (typeof STEPS)[number];

/** The three answers to "How do you work?". The first is the normal cockpit; the other two are Simple mode. */
export type Look = 'code' | 'ai' | 'none';
export const LOOKS: Look[] = ['code', 'ai', 'none'];

export const isSimpleLook = (look: Look | null): boolean => look === 'ai' || look === 'none';

export interface ProjectPick {
  path: string;
  name: string;
  /** Git state of the folder; the first task needs `repo` (a separate copy is made from it). */
  state: 'repo' | 'no_commits' | 'folder' | 'missing';
}

export type TaskState = 'idle' | 'running' | 'done' | 'failed' | 'skipped';

/** An agent that passed its test; the first task can use it. */
export interface ReadyAgent {
  id: string;
  label: string;
}

export interface Flow {
  step: StepId;
  look: Look | null;
  /** The agents that passed their test (the Set up agents list says Ready). */
  ready: ReadyAgent[];
  project: ProjectPick | null;
  task: TaskState;
  /** The welcome is over (finished or skipped): the host closes it and writes onboarding_done. */
  finished: boolean;
}

export type Action =
  | { type: 'look'; look: Look }
  | { type: 'agents'; ready: ReadyAgent[] }
  | { type: 'project'; project: ProjectPick | null }
  | { type: 'task'; task: TaskState }
  | { type: 'next' }
  | { type: 'back' }
  /** "Skip for now" on one step: go on without what the step gives. */
  | { type: 'skip' }
  /** "Skip setup": leave the whole welcome from any step. */
  | { type: 'skipAll' }
  | { type: 'go'; step: StepId };

export const stepIndex = (step: StepId): number => STEPS.indexOf(step);
export const isStep = (value: unknown): value is StepId => typeof value === 'string' && (STEPS as readonly string[]).includes(value);

export function initialFlow(overrides: Partial<Flow> = {}): Flow {
  return { step: 'welcome', look: null, ready: [], project: null, task: 'idle', finished: false, ...overrides };
}

/** What a step needs before "Continue" is on. Skip is always on. */
export function canContinue(flow: Flow): boolean {
  switch (flow.step) {
    case 'welcome':
      return flow.look !== null;
    case 'agents':
      return flow.ready.length > 0;
    case 'project':
      return flow.project !== null && flow.project.state === 'repo';
    case 'task':
      return flow.task === 'done';
    default:
      return true;
  }
}

function advance(flow: Flow): Flow {
  const i = stepIndex(flow.step);
  // The last step's button closes the welcome.
  if (i >= STEPS.length - 1) return { ...flow, finished: true };
  return { ...flow, step: STEPS[i + 1] };
}

const sameAgents = (a: ReadyAgent[], b: ReadyAgent[]): boolean => a.length === b.length && a.every((x, i) => x.id === b[i].id);

export function reduce(flow: Flow, action: Action): Flow {
  switch (action.type) {
    case 'look':
      return { ...flow, look: action.look };
    case 'agents':
      // The doctor reports its list again after every check; the same agents change nothing.
      return sameAgents(flow.ready, action.ready) ? flow : { ...flow, ready: action.ready };
    case 'project':
      // A different folder starts the first task over.
      return { ...flow, project: action.project, task: flow.project?.path === action.project?.path ? flow.task : 'idle' };
    case 'task':
      return { ...flow, task: action.task };
    case 'next':
      return canContinue(flow) ? advance(flow) : flow;
    case 'skip': {
      const skipped = flow.step === 'task' && flow.task !== 'done' ? { ...flow, task: 'skipped' as const } : flow;
      return advance(skipped);
    }
    case 'skipAll':
      return { ...flow, finished: true };
    case 'back': {
      const i = stepIndex(flow.step);
      return i === 0 ? flow : { ...flow, step: STEPS[i - 1] };
    }
    case 'go':
      return { ...flow, step: action.step };
  }
}

/**
 * Where a restart or a reopen lands. A saved step is kept when what it needs is still there; the first
 * task needs a folder, so without one the person lands on the folder step instead. A reopen
 * (`fresh`) always starts at the top.
 */
export function resumeStep(saved: string | null | undefined, haveProject: boolean, fresh = false): StepId {
  if (fresh || !isStep(saved)) return 'welcome';
  if (saved === 'task' && !haveProject) return 'project';
  return saved;
}

/** What the cockpit asks Rust at start (`first_run_state`). */
export interface FirstRunState {
  /** Should the welcome be on screen? */
  show: boolean;
  /** True when the person never decided (`onboarding_done` absent). */
  auto: boolean;
  step: string | null;
}

/**
 * The welcome shows when Rust says so. An automatic show (never decided) is vetoed when this install
 * has already had a pane: an existing user who updates must not meet a welcome. A requested show
 * (palette, Settings) is never vetoed.
 */
export function shouldShowWelcome(state: FirstRunState | null, everSpawned: boolean, paneCount: number): boolean {
  if (!state || !state.show) return false;
  if (!state.auto) return true;
  return !everSpawned && paneCount === 0;
}

// ---- what each choice writes (all through the ordinary settings patch) ----

export const lookPatch = (look: Look) => ({ simple_mode: isSimpleLook(look) });
export const stepPatch = (step: StepId) => ({ onboarding_step: step });
export const projectPatch = (project: ProjectPick | null) => ({ onboarding_project: project ? project.path : null });
export const agentPatch = (ready: ReadyAgent[]) => ({ onboarding_agent: ready.length > 0 ? ready[0].id : null });
/** Finished or skipped: never shown again on its own. The saved step and folder are dropped. */
export const finishPatch = () => ({ onboarding_done: true, onboarding_step: null, onboarding_project: null, onboarding_agent: null });
/** Asked for again (palette, Settings): `false` makes every window show it. */
export const reopenPatch = () => ({ onboarding_done: false, onboarding_step: 'welcome' });

/** The Simple/normal look implied by the saved `simple_mode`, for pre-selecting step 1 on a reopen. */
export const lookFromSettings = (simpleMode: boolean | undefined): Look | null => (simpleMode === undefined ? null : simpleMode ? 'ai' : 'code');

/** Agents that passed their test, in list order. */
export function readyAgents<T extends { id: string; label: string; state: string }>(rows: T[]): ReadyAgent[] {
  return rows.filter((r) => r.state === 'ready').map((r) => ({ id: r.id, label: r.label }));
}
