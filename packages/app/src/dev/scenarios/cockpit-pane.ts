import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

const s: HarnessScenario = { name: 'cockpit-pane', route: 'cockpit', select: PANE_CLAUDE };
export default s;
