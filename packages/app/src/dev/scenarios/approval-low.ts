import type { HarnessScenario } from '../types';

// Inbox with one low-risk approval card (the default example prompt).
const s: HarnessScenario = { name: 'approval-low', route: 'cockpit', after: async (dom) => { await dom.sleep(800); } };
export default s;
