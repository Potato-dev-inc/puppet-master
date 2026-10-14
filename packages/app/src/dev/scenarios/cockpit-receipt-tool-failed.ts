import type { HarnessScenario } from '../types';
import { PANE_CODEX } from '../fixtures';

// A Codex run whose own tool failed ("workspace command runner failed to start"): the receipt says so
// instead of looking like any other unchecked run (VERIFY-R10 NEW #7).
const s: HarnessScenario = {
  name: 'cockpit-receipt-tool-failed',
  route: 'cockpit',
  select: PANE_CODEX,
  runs: {
    [PANE_CODEX]: {
      handle: 'run_0af0a3c3',
      status: 'completed',
      receipt: {
        backend: 'codex',
        model: 'gpt-6-luna',
        read_only: 'off',
        checks: 'none configured',
        result_capture: 'inferred',
        not_verified: ['no checks configured', 'result read from terminal screen', 'model not confirmed', 'agent reported a tool failure'],
        outcome: 'completed_unchecked',
        build_id: '404e20d',
      },
    },
  },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Receipt');
    await dom.sleep(1500);
  },
};
export default s;
