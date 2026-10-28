import { MORNING_STATE } from '../night-fixtures';
import type { HarnessScenario } from '../types';

// The Morning page: "4 of 5 done, tests green. 1 needs you." Four rows to keep or throw away, one to look at.
const s: HarnessScenario = {
  name: 'night-morning-4-of-5',
  route: 'cockpit',
  night: { state: MORNING_STATE },
};
export default s;
