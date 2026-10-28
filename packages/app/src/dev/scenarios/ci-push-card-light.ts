import type { HarnessScenario } from '../types';
import { PUSH_CARD } from './ci-push-card';

// The same card in the light theme.
const s: HarnessScenario = {
  name: 'ci-push-card-light',
  route: 'cockpit',
  theme: 'light',
  questions: [PUSH_CARD],
};
export default s;
