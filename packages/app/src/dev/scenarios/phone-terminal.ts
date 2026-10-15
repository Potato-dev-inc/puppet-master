import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: an agent tab with its terminal (mocked screen) and the Keys panel open.
const s: HarnessScenario = {
  name: 'phone-terminal',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: {
    activeTab: PANE_CLAUDE,
    events: [{ name: 'loop-signal', data: { pane_id: PANE_CLAUDE, signal: { kind: 'repeated_error', summary: 'the same error 4 times', evidence_lines: [] } } }],
  },
  after: async (dom) => {
    await dom.waitFor('.xterm');
    await dom.sleep(1500);
    await dom.click('.terminal-mobile-special-toggle');
    await dom.sleep(800);
  },
};
export default s;
