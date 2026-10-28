import type { HarnessScenario } from '../types';

// The Focus pill under the sidebar header. It shows only while `focus_state` answers "on": the mocked Focus
// (`dev/focus-fixtures.ts`) starts on, 31 minutes left, holding 3 updates.
const s: HarnessScenario = {
  name: 'focus-pill',
  route: 'cockpit',
  focus: { on: true },
  after: async (dom) => {
    await dom.waitFor('.ck-focus-pill');
    await dom.sleep(600);
  },
};
export default s;
