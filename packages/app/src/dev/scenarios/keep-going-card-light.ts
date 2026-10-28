import { LIMIT_CARD } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// The same card in the light theme.
const s: HarnessScenario = {
  name: 'keep-going-card-light',
  route: 'cockpit',
  theme: 'light',
  questions: [LIMIT_CARD],
};
export default s;
