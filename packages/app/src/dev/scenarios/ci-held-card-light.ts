import type { HarnessScenario } from '../types';
import { HELD_CARD } from './ci-held-card';

// The same card in the light theme.
const s: HarnessScenario = {
  name: 'ci-held-card-light',
  route: 'cockpit',
  theme: 'light',
  questions: [HELD_CARD],
};
export default s;
