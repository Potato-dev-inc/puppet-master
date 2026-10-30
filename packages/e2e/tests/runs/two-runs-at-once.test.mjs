// Two runs at the same time in two projects: two panes, each with its own answer, none mixed up.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'runs.two-runs-at-once',
  title: 'Two runs at once get two panes and two separate answers',
  area: 'runs',
  tags: ['mcp'],
  edge: true,
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const alpha = t.project('alpha');
  const beta = t.project('beta');
  const m = mcp(t);

  await t.step('Start two runs without waiting for the first');
  const a = (await m.run({ agent_type: 'nimbus', project_path: alpha, task: 'job for alpha' })).body.handle;
  const b = (await m.run({ agent_type: 'nimbus', project_path: beta, task: 'job for beta' })).body.handle;
  t.expect.ok(a && b && a !== b, 'two different handles');

  await t.step('Two panes are listed, one per run');
  await t.expect.count(PANE_ROW, 2);
  await t.expect.visible('job for alpha', { within: 'css=.ck-side' });
  await t.expect.visible('job for beta', { within: 'css=.ck-side' });

  await t.step('Each run finishes in its own pane');
  const doneA = firstRun(await m.wait(alpha, a));
  const doneB = firstRun(await m.wait(beta, b));
  t.expect.equal(doneA.status, 'completed');
  t.expect.equal(doneB.status, 'completed');
  t.expect.ok(doneA.pane_id !== doneB.pane_id, 'different panes');
  t.expect.equal((await t.panes()).length, 2, 'both panes are still there (keep_pane)');
});
