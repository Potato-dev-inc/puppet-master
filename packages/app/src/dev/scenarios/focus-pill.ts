import type { HarnessScenario } from '../types';

// The Focus pill under the sidebar header. It shows only while `focus_state` answers "on". The dev harness does not
// answer Focus commands yet (no `focus` fixture in `dev/types.ts` or `mock-tauri.ts`), so this shows the sidebar
// with no pill until that mock is wired.
const s: HarnessScenario = {
  name: 'focus-pill',
  route: 'cockpit',
  after: async (dom) => {
    await dom.waitFor('.ck-side');
    await dom.sleep(600);
  },
};
export default s;
