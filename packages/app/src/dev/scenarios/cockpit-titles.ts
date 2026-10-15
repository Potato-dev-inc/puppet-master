import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_CLAUDE, PANE_CODEX, PANE_OPENCODE } from '../fixtures';

/** Sidebar rows and the pane header with auto-derived titles (agent label becomes the secondary line). */
const TITLES: Record<string, string> = {
  [PANE_CLAUDE]: 'review the payments migration',
  [PANE_CODEX]: 'auth refactor',
  [PANE_OPENCODE]: 'fix flaky docs build',
};

const s: HarnessScenario = {
  name: 'cockpit-titles',
  route: 'cockpit',
  select: PANE_CODEX,
  panes: EXAMPLE_PANES.map((p) => (TITLES[p.id] ? { ...p, title: TITLES[p.id] } : p)),
};
export default s;
