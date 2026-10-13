import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Running pane with a dev server: "Preview :5173" chip in the header, globe on its sidebar row.
const s: HarnessScenario = { name: 'live-preview', route: 'cockpit', select: PANE_CODEX, after: async (dom) => { await dom.sleep(900); } };
export default s;
