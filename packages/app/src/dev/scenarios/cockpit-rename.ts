import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_CLAUDE, PANE_CODEX } from '../fixtures';

/** The header title in rename mode (double-click). */
const s: HarnessScenario = {
  name: 'cockpit-rename',
  route: 'cockpit',
  select: PANE_CODEX,
  panes: EXAMPLE_PANES.map((p) =>
    p.id === PANE_CODEX ? { ...p, title: 'auth refactor' } : p.id === PANE_CLAUDE ? { ...p, title: 'review the payments migration' } : p,
  ),
  after: async (dom) => {
    await dom.waitFor('.ck-title--editable');
    const el = document.querySelector('.ck-title--editable');
    el?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await dom.sleep(500);
  },
};
export default s;
