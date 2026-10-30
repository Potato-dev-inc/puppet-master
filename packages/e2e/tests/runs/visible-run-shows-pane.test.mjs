// An agent run started through the bridge (what an MCP client does) opens a visible pane that the cockpit lists, and the
// answer comes back with a receipt that says what was and was not verified.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'runs.visible-run-shows-pane',
  title: 'A run started over MCP opens a visible pane and returns an honest receipt',
  area: 'runs',
  tags: ['smoke', 'mcp'],
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('shop');
  const m = mcp(t);

  await t.step('An MCP client starts a run with run_agent');
  const started = await m.run({ agent_type: 'nimbus', project_path: project, task: 'tidy the shop page' });
  t.expect.equal(started.status, 200);
  const handle = started.body.handle;
  t.expect.ok(handle, 'the run has a handle');

  await t.step('The pane shows up in the sidebar, named after the task');
  await t.expect.count(PANE_ROW, 1);
  await t.expect.visible('tidy the shop page', { within: 'css=.ck-side' });
  await t.expect.visible('Nimbus · shop', { within: 'css=.ck-side' });

  await t.step('wait_agents returns the finished run');
  const done = firstRun(await m.wait(project, handle));
  t.expect.equal(done.status, 'completed');
  t.expect.includes(done.result, 'all done: first job');

  await t.step('The receipt says which agent ran and what was NOT verified');
  const r = done.receipt;
  t.expect.equal(r.agent_definition.id, 'nimbus');
  t.expect.equal(r.outcome, 'completed_unchecked');
  t.expect.equal(r.result_capture, 'inferred');
  t.expect.equal(r.fallback, null);
  t.expect.equal(done.verified, false);
  t.expect.ok(r.not_verified.some((s) => /read from terminal screen/.test(s)), 'it says the result came from the screen');
  t.expect.ok(r.not_verified.some((s) => /no checks configured/.test(s)), 'it says no checks ran');

  await t.step('The finished run keeps its pane, idle, and the screen shows its reply');
  await t.expect.visible('Idle', { within: 'css=.ck-side' });
  await t.click('tidy the shop page', { within: 'css=.ck-side' });
  await t.click('exact=Terminal');
  await t.expect.text('reply: all done: first job');
});
