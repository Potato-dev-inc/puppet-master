// followup_task: the next turn runs on the same worker and the same pane, and the run view shows the new answer.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'runs.followup',
  title: 'followup_task runs a second turn in the same pane',
  area: 'runs',
  tags: ['mcp'],
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('twice');
  const m = mcp(t);

  await t.step('The first turn');
  const handle = (await m.run({ agent_type: 'nimbus', project_path: project, task: 'first job' })).body.handle;
  const first = firstRun(await m.wait(project, handle));
  t.expect.includes(first.result, 'all done: first job');
  await t.expect.count(PANE_ROW, 1);

  await t.step('Send a follow-up on the same handle');
  const f = await m.followup(project, handle, 'now the second job');
  t.expect.equal(f.status, 200);

  await t.step('The second answer arrives from the same pane');
  await t.expect.eventually(async () => /second job done/.test(firstRun(await m.wait(project, handle, ['completed', 'failed'], 1000))?.result || ''), { timeout: 30000, message: 'expected the second answer' });
  const second = firstRun(await m.wait(project, handle));
  t.expect.equal(second.pane_id, first.pane_id, 'the same pane');
  t.expect.equal(second.status, 'completed');
  const prompts = t.fake.events().filter((e) => e.ev === 'prompt').map((e) => e.text);
  t.expect.equal(prompts.length, 2, 'the agent got two prompts');
  await t.expect.count(PANE_ROW, 1);
  await t.click('first job', { within: 'css=.ck-side' });
  await t.click('exact=Terminal');
  await t.expect.text('reply: second job done');
});
