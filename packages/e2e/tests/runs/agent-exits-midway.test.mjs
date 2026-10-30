// The agent says something and then quits with an error code in the middle of a run.
import { mcp, firstRun, RUN_SCRIPT, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.agent-exits-midway',
  title: 'A run whose agent quits after speaking fails and keeps what it said',
  area: 'runs',
  tags: ['mcp'],
  edge: true,
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('quits');
  const m = mcp(t);

  await t.step('Start a run whose agent says bye and quits');
  const handle = (await m.run({ agent_type: 'nimbus', project_path: project, task: 'exit now' })).body.handle;

  await t.step('The run fails: the agent is gone');
  const view = firstRun(await m.wait(project, handle));
  t.expect.equal(view.status, 'failed');
  t.expect.equal(view.error.code, 'AGENT_EXITED');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'exit').length, 1);

  await t.step('What the agent said before it quit is in the error, for the caller to read');
  t.expect.includes(view.error.context.last_screen, 'reply: bye');

  await t.step('The cockpit shows the pane as an error, needing you');
  await t.expect.visible('Error', { within: 'css=.ck-side', timeout: 20000 });
  t.expect.equal((await t.panes())[0].status, 'error');
});
