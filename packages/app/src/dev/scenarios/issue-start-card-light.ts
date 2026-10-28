import type { HarnessScenario } from '../types';
import { START_CARD } from './issue-start-card';

// The same card in the light theme.
const s: HarnessScenario = {
  name: 'issue-start-card-light',
  route: 'cockpit',
  theme: 'light',
  questions: [START_CARD],
};
export default s;
