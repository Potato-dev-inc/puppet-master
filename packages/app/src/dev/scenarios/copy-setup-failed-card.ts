import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// A failed setup is said in plain words on the Changes tab, with the last lines of the output one click away.
const s: HarnessScenario = {
  name: 'copy-setup-failed-card',
  route: 'cockpit',
  select: PANE_CLAUDE,
  ship: {
    worktree: {
      setup: {
        status: 'failed',
        copied: ['.env'],
        not_copied: [],
        steps: [{ command: 'npm ci', ok: false, exit_code: 1, timed_out: false, tail: 'npm ERR! code ELIFECYCLE\nnpm ERR! errno 1\nnpm ERR! shop@1.0.0 postinstall: node scripts/setup.js', duration_ms: 4200 }],
        duration_ms: 4300,
      },
    },
  },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-setup');
    await dom.click('.ck-changes-setup summary');
    await dom.sleep(500);
  },
};
export default s;
