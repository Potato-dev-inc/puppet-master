// cancel_agent stops a turn that is still running. The run ends as cancelled and the agent's pane is stopped working.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.cancel-run',
  title: 'cancel_agent stops a running turn and the run says it was cancelled',
  area: 'runs',
  tags: ['mcp'],
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('slow');
  const m = mcp(t);

  await t.step('Start a slow run');
  const started = await m.run({ agent_type: 'nimbus', project_path: project, task: 'slow job please' });
  const handle = started.body.handle;
  await t.expect.visible('Working', { within: 'css=.ck-side' });
  await t.fake.waitEvent('prompt');

  await t.step('Cancel it while it works');
  const c = await m.cancel(project, handle);
  t.expect.equal(c.status, 200);

  await t.step('The run reports that it was cancelled');
  const view = firstRun(await m.wait(project, handle));
  t.expect.equal(view.receipt.outcome, 'cancelled');
  t.expect.ok(['interrupted', 'cancelled'].includes(view.status), `status is ${view.status}`);
  t.expect.ok(!/late answer/.test(view.result || ''), 'the slow answer never arrived');

  await t.step('The agent was interrupted and its pane is no longer working');
  await t.fake.waitEvent('interrupted');
  await t.expect.eventually(async () => (await t.panes())[0]?.status !== 'running', { message: 'expected the pane to stop working' });
  await t.expect.noText('Working');
});
