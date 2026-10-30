// A run whose agent stops to ask is "needs_input" for the MCP client, and a card in the Inbox for the person. Answering the
// card carries the run on.
import { mcp, firstRun, RUN_SCRIPT, guarded, waitBlocked } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.needs-input-answer-in-ui',
  title: 'A run that needs an answer shows a card and a click on Allow once finishes it',
  area: 'runs',
  tags: ['smoke', 'mcp'],
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('ask');
  const m = mcp(t);

  await t.step('Start a run whose agent asks before running a command');
  const handle = (await m.run({ agent_type: 'nimbus', project_path: project, task: 'ask first please' })).body.handle;

  await t.step('wait_agents reports needs_input with the question and its options');
  const blocked = await waitBlocked(t, m, project, handle);
  t.expect.equal(blocked.status, 'needs_input');
  t.expect.equal(blocked.prompt.kind, 'command_approval');
  t.expect.includes(blocked.prompt.summary, 'echo hi');
  t.expect.equal(blocked.prompt.options.map((o) => o.id), ['allow_once', 'deny']);

  await t.step('The cockpit shows it as needing you, in the Inbox');
  await t.expect.visible('Needs you', { within: 'css=.ck-side' });
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('One thing needs you');
  await t.expect.visible('exact=Allow once');
  await t.expect.visible('exact=Deny');

  await t.step('Click Allow once');
  await t.click('exact=Allow once');

  await t.step('The agent got the answer and the run completes');
  await t.fake.waitEvent('approval_answer');
  const done = firstRun(await m.wait(project, handle));
  t.expect.equal(done.status, 'completed');
  t.expect.includes(done.result, 'the command ran');
  await t.expect.text('Nothing needs you');
});
