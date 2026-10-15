import type { HarnessScenario } from '../types';
import { PANE_CLAUDE, PANE_CODEX } from '../fixtures';

// Two agents that stopped with an error: each card says why, in one sentence, with one fix button.
const now = Date.now();
const stopped = (id: string, agent: string, cwd: string) => ({
  id,
  agent_type: agent,
  pid: 1,
  status: 'error' as const,
  created_at: now - 5 * 60_000,
  last_output_at: now - 60_000,
  cwd,
  cols: 120,
  rows: 32,
});

const s: HarnessScenario = {
  name: 'cockpit-inbox-error',
  route: 'cockpit',
  panes: [stopped(PANE_CLAUDE, 'claude', 'C:/Users/dev/work/puppet-master'), stopped(PANE_CODEX, 'codex', 'C:/Users/dev/work/billing-service')],
  runs: {
    [PANE_CLAUDE]: { handle: 'run_1', status: 'failed', receipt: null, error: { code: 'WORKSPACE_TRUST_DECLINED' } },
    [PANE_CODEX]: { handle: 'run_2', status: 'failed', receipt: null, error: { code: 'AGENT_EXITED' } },
  },
};
export default s;
