// A run whose agent crashes is a failed run with a reason, and the pane shows the Error state.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.agent-crashes',
  title: 'A run whose agent crashes fails with AGENT_EXITED and the pane shows Error',
  area: 'runs',
  tags: ['mcp'],
  edge: true,
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('boom');
  const m = mcp(t);

  await t.step('Start a run whose agent panics on the task');
  const handle = (await m.run({ agent_type: 'nimbus', project_path: project, task: 'crash now' })).body.handle;

  await t.step('The run fails and says the agent exited');
  const view = firstRun(await m.wait(project, handle));
  t.expect.equal(view.status, 'failed');
  t.expect.equal(view.error.code, 'AGENT_EXITED');
  t.expect.includes(view.error.context.last_screen, 'boom');
  t.expect.ok(view.receipt.outcome !== 'completed_unchecked', 'it is not reported as done');

  await t.step('The pane is in the Error state in the sidebar');
  await t.expect.visible('Error', { within: 'css=.ck-side' });
  t.expect.equal((await t.panes())[0].status, 'error');
});
