import { CONFLICT_STATE } from '../night-fixtures';
import type { HarnessScenario } from '../types';

// The Morning page after a Keep that conflicted: that row says so and offers only Throw away.
const s: HarnessScenario = {
  name: 'night-morning-conflict',
  route: 'cockpit',
  night: { state: CONFLICT_STATE },
};
export default s;
