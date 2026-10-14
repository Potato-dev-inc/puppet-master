import { teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// Updates may have been missed (and the relay can't be reached right now).
const s: HarnessScenario = {
  name: 'cockpit-team-gap',
  route: 'cockpit',
  team: teamState({ missing: 3, link: 'offline' }),
};
export default s;
