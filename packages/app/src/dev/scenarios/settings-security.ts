import type { HarnessScenario } from '../types';

// Settings > Bridge & MCP > Security: the two switches (shown at the recommended values A on, B off) and the
// read-only list of recent approvals and control changes.
const now = Date.now();
const min = 60_000;
const s: HarnessScenario = {
  name: 'settings-security',
  route: 'settings',
  settingsTab: 'bridge',
  window: { width: 1440, height: 1100 },
  authority: { ask_before_agent_control: true, agents_may_merge: false },
  audit: [
    { seq: 7, ts_ms: now - 2 * min, kind: 'consent', who: 'desktop', summary: 'You allowed Claude Code to control Codex, once' },
    { seq: 6, ts_ms: now - 41 * min, kind: 'approval', who: 'device', summary: 'A phone answered from its lock screen: Allowed once' },
    { seq: 5, ts_ms: now - 95 * min, kind: 'merge', who: 'agent', summary: 'An agent tried to merge a separate copy; it was not allowed' },
    { seq: 4, ts_ms: now - 3 * 60 * min, kind: 'shell_exec', who: 'agent', summary: 'Ran a command in a shell pane' },
    { seq: 3, ts_ms: now - 5 * 60 * min, kind: 'gate_dismissal', who: 'local', summary: 'Puppet Master closed a start-up screen on its own: An update notice' },
    { seq: 2, ts_ms: now - 26 * 60 * min, kind: 'pairing', who: 'desktop', summary: 'You made a pairing code for a phone' },
    { seq: 1, ts_ms: now - 27 * 60 * min, kind: 'setting', who: 'desktop', summary: "Ask me before an agent controls a pane it didn't start: turned on" },
  ],
  after: async (dom) => {
    await dom.waitFor('[data-row="security-recent-approvals"] .st-audit-row');
    await dom.sleep(300);
    document.querySelector('[data-row="security-ask-before-control"]')?.scrollIntoView({ block: 'start' });
  },
};
export default s;
