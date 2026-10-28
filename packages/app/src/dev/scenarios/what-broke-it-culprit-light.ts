import { CULPRIT } from '../what-broke-it-result-fixtures';
import type { HarnessScenario } from '../types';

// The culprit card in light theme.
const s: HarnessScenario = {
  name: 'what-broke-it-culprit-light',
  route: 'cockpit',
  theme: 'light',
  whatBrokeItResult: { finding: CULPRIT },
};
export default s;
