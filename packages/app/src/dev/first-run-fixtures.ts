// What the mocked backend answers for the first-run welcome (harness scenarios only).
import cases from '../components/simple/folder-cases.json';
import { folderVerdict, type KnownDirs } from '../components/simple/folder-guard';
import type { Look, StepId } from '../components/firstrun/model';
import type { RecentProject } from '../lib/ship-api';

export interface FirstRunScenario {
  /** Where the welcome starts (the saved step). Default: the first. */
  step?: StepId;
  /** The look already chosen (a reopen). */
  look?: Look;
  /** Folder and agent saved from an earlier visit, so a later step can show its content. */
  project?: string;
  agent?: string;
  /** What the first task does once it runs. */
  task?: 'running' | 'done' | 'failed';
}

const KNOWN = cases.known as KnownDirs;
const now = Date.now();

/** Recent projects with one folder the guard refuses (Documents). */
export const FIRST_RUN_RECENT: RecentProject[] = [
  { path: 'D:\\code\\shop', name: 'shop', state: 'repo', last_used_ms: now - 60_000 },
  { path: 'D:\\code\\landing-page', name: 'landing-page', state: 'folder', last_used_ms: now - 3_600_000 },
  { path: 'C:\\Users\\Ana\\Documents', name: 'Documents', state: 'folder', last_used_ms: now - 86_400_000 },
  { path: 'D:\\code\\old-experiment', name: 'old-experiment', state: 'missing', last_used_ms: now - 9 * 86_400_000 },
];

// The shape of a real answer from the live run: paragraphs, a list, bold and code marks.
export const TASK_ANSWER = [
  'This is a tiny plant shop. The README says it lists plants and lets a visitor add them to a cart, but the code only does the listing part so far.',
  '',
  'The project has three files:',
  '- **`README.md`** is a two-line description.',
  '- **`package.json`** names the project `tiny-shop` and defines one script, `npm start`, which runs `node src/server.js`.',
  '- **`src/server.js`** is a plain Node `http` server on port 3000 with two hard-coded plants.',
  '',
  "I didn't change any files.",
].join('\n');

const RECEIPT = {
  backend: 'claude',
  read_only: 'enforced',
  checks: 'none configured',
  result_capture: 'authoritative',
  not_verified: [],
  outcome: 'completed_unchecked',
  build_id: 'harness',
  worktree: { path: 'D:\\code\\shop\\.puppet-master\\worktrees\\a1b2', branch: 'pm/task-1', base: 'main', state: 'discarded', files_changed: 0, additions: 0, deletions: 0 },
};

/** The settings the mocked store starts with, besides the theme. */
export function firstRunStore(sc: FirstRunScenario | undefined): Record<string, unknown> {
  if (!sc) return { onboarding_done: true };
  return {
    ...(sc.look ? { simple_mode: sc.look !== 'code' } : {}),
    ...(sc.project ? { onboarding_project: sc.project } : {}),
    ...(sc.agent ? { onboarding_agent: sc.agent } : {}),
  };
}

export function firstRunMock(sc: FirstRunScenario | undefined, cmd: string, a: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!sc) return { handled: false };
  const yes = (value: unknown) => ({ handled: true, value });
  switch (cmd) {
    case 'first_run_state':
      return yes({ show: true, auto: false, step: sc.step ?? null });
    case 'plain_check_folder': {
      const verdict = folderVerdict(String(a.path ?? ''), KNOWN);
      return yes(verdict === 'ok' ? { ok: true } : { ok: false, code: 'FOLDER_TOO_BROAD', message: verdict });
    }
    case 'project_recent':
      return yes(FIRST_RUN_RECENT);
    case 'first_run_start_task':
      return yes({ project: a.project, handle: 'task-1', pane_id: null, enforced: true });
    case 'first_run_task_status':
      if (sc.task === 'failed') {
        return yes({ status: 'failed', result: null, error: { code: 'AGENT_EXITED', message: 'exited' }, receipt: null, pane_id: null });
      }
      if (sc.task === 'running') return yes({ status: 'running', result: null, error: null, receipt: null, pane_id: null });
      return yes({ status: 'completed', result: TASK_ANSWER, error: null, receipt: RECEIPT, pane_id: null });
    default:
      return { handled: false };
  }
}
