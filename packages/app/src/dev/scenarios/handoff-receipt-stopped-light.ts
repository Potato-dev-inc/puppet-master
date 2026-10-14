import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// (Light theme) The receipt of a run a person stopped at a usage limit: "Stopped at a usage limit (resets 14:05)".
const resetAt = new Date();
resetAt.setHours(14, 5, 0, 0);
const s: HarnessScenario = {
  name: 'handoff-receipt-stopped-light',
  route: 'cockpit',
  theme: 'light',
  select: PANE_CLAUDE,
  runs: {
    [PANE_CLAUDE]: {
      handle: 'run_5c21d0aa',
      status: 'cancelled',
      receipt: {
        backend: 'claude',
        model: 'claude-opus-5-5',
        read_only: 'off',
        checks: 'none configured',
        result_capture: 'missing',
        not_verified: ['no checks configured'],
        outcome: 'cancelled',
        build_id: '404e20d',
        usage_limit: { state: 'stopped', window: 'session limit', reset_at_ms: resetAt.getTime(), reset_text: '2:05pm (Asia/Taipei)' },
      },
    },
  },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Receipt');
    await dom.sleep(1500);
  },
};
export default s;
