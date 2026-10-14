import type { HarnessScenario } from '../types';
import { PANE_CODEX } from '../fixtures';

// A running pane: the header state word and the pane palette row must say "Working".
const s: HarnessScenario = { name: 'cockpit-pane-working', route: 'cockpit', select: PANE_CODEX };
export default s;
