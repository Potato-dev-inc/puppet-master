import type { HarnessScenario } from '../types';
import { FIX_CARD } from './ci-fix-card';

// The same card in the light theme.
const s: HarnessScenario = {
  name: 'ci-fix-card-light',
  route: 'cockpit',
  theme: 'light',
  questions: [FIX_CARD],
};
export default s;
