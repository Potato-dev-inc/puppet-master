import { CULPRIT } from '../what-broke-it-result-fixtures';
import type { HarnessScenario } from '../types';

// The result card for the culprit turn: the title, the broken test line, Show what it changed, Undo only this turn.
const s: HarnessScenario = {
  name: 'what-broke-it-culprit',
  route: 'cockpit',
  whatBrokeItResult: { finding: CULPRIT },
};
export default s;
