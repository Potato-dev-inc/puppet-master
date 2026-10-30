// keep_pane is off by default: a pane the run made closes when the run is done, and the answer stays readable.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'runs.pane-closes-after-run',
  title: 'A run without keep_pane closes its pane when it is done but the answer stays',
  area: 'runs',
  tags: ['mcp'],
  edge: true,
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('tmp-job');
  const m = mcp(t);

  await t.step('Start a run without keep_pane (it is off by default)');
  const started = await m.run({ agent_type: 'nimbus', project_path: project, task: 'one-off job', keep_pane: false });
  const handle = started.body.handle;
  await t.expect.count(PANE_ROW, 1);

  await t.step('The run finishes');
  const done = firstRun(await m.wait(project, handle));
  t.expect.equal(done.status, 'completed');
  const paneId = done.pane_id;

  await t.step('Its pane closes by itself and leaves the sidebar');
  await t.expect.eventually(async () => !(await t.panes()).some((p) => p.id === paneId), { timeout: 30000, message: 'expected the run pane to close' });
  await t.expect.count(PANE_ROW, 0);

  await t.step('The answer can still be read from the stored transcript');
  const again = firstRun(await m.wait(project, handle));
  t.expect.equal(again.status, 'completed');
  t.expect.includes(again.result, 'all done: first job');
});
