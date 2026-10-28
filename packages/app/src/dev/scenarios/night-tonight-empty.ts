import { EMPTY_STATE } from '../night-fixtures';
import type { HarnessScenario } from '../types';

// Tonight with nothing planned yet: Labs on, no run, Start on. The sheet offers the saved plan and stop time.
const s: HarnessScenario = {
  name: 'night-tonight-empty',
  route: 'cockpit',
  night: { state: EMPTY_STATE },
};
export default s;
