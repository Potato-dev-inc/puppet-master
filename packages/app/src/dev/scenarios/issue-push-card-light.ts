import type { HarnessScenario } from '../types';
import { PUSH_CARD } from './issue-push-card';

// The same card in the light theme.
const s: HarnessScenario = {
  name: 'issue-push-card-light',
  route: 'cockpit',
  theme: 'light',
  questions: [PUSH_CARD],
};
export default s;
