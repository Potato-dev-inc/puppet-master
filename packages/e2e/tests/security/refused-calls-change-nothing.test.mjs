// Calls that change things (start a pane, run an agent, write into a pane) are refused for a remote caller, and nothing changes.
import { rawBridge, mcp, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'security.refused-calls-change-nothing',
  title: 'Remote-looking calls that start panes, run agents or type are refused and change nothing',
  area: 'security',
  tags: ['security'],
  edge: true,
};

export const run = guarded(async (t) => {
  const project = t.project('guarded');
  const remote = { 'X-Forwarded-For': '203.0.113.9' };
  const pane = await t.spawnPane('nimbus');

  await t.step('Start a pane, forwarded: refused');
  const spawn = await rawBridge(t, { method: 'POST', path: '/panes', headers: remote, body: { agent_type: 'nimbus', cwd: project } });
  t.expect.equal(spawn.status, 401);

  await t.step('Type into the existing pane, from a foreign web page: refused');
  const typed = await rawBridge(t, { method: 'POST', path: `/panes/${pane.id}/input`, headers: { origin: 'https://evil.example' }, body: { text: 'rm -rf everything', append_newline: true } });
  t.expect.equal(typed.status, 401);

  await t.step('Close the pane, with a foreign Host: refused');
  const closed = await rawBridge(t, { method: 'DELETE', path: `/panes/${pane.id}`, headers: { host: 'evil.example.com' } });
  t.expect.equal(closed.status, 401);

  await t.step('Run an agent, forwarded, even with an MCP session: refused');
  const m = mcp(t);
  const run = await m.call('POST', '/agents/run', { agent_type: 'nimbus', project_path: project, task: 'do harm', wait_ms: 1, background: true }, remote);
  t.expect.equal(run.status, 401);

  await t.step('Nothing changed: still one pane, nothing typed, no second agent');
  t.expect.equal((await t.panes()).length, 1);
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'the agent never got a prompt');
  await t.expect.count('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', 1);
});
