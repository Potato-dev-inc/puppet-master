import { TONIGHT_FILLED } from '../night-fixtures';
import type { HarnessScenario } from '../types';

// Tonight with a saved setup: Codex is ticked, the stop time and the keep-awake answer are kept, two work at once.
const s: HarnessScenario = {
  name: 'night-tonight-filled',
  route: 'cockpit',
  night: { state: TONIGHT_FILLED },
};
export default s;
