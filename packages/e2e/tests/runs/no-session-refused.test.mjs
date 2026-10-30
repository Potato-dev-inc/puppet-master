// The agent routes need an MCP session. A plain call (no X-Puppet-Master-Session) is refused and starts nothing.
import { rawBridge, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.no-session-refused',
  title: 'run_agent without an MCP session id is refused and starts nothing',
  area: 'runs',
  tags: ['mcp', 'security'],
  edge: true,
};

export const run = guarded(async (t) => {
  const project = t.project('nosession');

  await t.step('Call /agents/run with the secret but no session id');
  const r = await rawBridge(t, { method: 'POST', path: '/agents/run', body: { agent_type: 'nimbus', project_path: project, task: 'hello' } });
  t.expect.equal(r.status, 400);
  t.expect.equal(r.body.code, 'SESSION_REQUIRED');

  await t.step('Waiting for a run has the same rule');
  const w = await rawBridge(t, { method: 'POST', path: '/agents/wait', body: { project_path: project, handles: ['x'] } });
  t.expect.equal(w.body.code, 'SESSION_REQUIRED');

  await t.step('Nothing was started');
  t.expect.equal((await t.panes()).length, 0);
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'start').length, 0);
});
