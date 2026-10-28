import { WAITING_STATE } from '../night-fixtures';
import type { HarnessScenario } from '../types';

// A run has a task waiting for a plan with usage left, and the other task working.
const s: HarnessScenario = {
  name: 'night-waiting',
  route: 'cockpit',
  night: { state: WAITING_STATE },
};
export default s;
