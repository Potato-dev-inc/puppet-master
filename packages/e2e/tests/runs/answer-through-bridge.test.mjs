// answer_prompt: the MCP client answers a blocked run itself. "deny" is delivered to the agent and the run goes on.
import { mcp, firstRun, RUN_SCRIPT, guarded, askingRun } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.answer-through-bridge',
  title: 'answer_prompt with "deny" refuses the command and the run finishes',
  area: 'runs',
  tags: ['mcp'],
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const m = mcp(t);
  const { project, handle, blocked } = await askingRun(t, m, 'answer-by-bridge');
  t.expect.equal(blocked.status, 'needs_input');

  await t.step('Answer with deny, by the prompt id from the run view');
  const a = await m.answer({ project_path: project, handle, prompt_id: blocked.prompt.id, choice: 'deny' });
  t.expect.equal(a.status, 200, `answer_prompt said ${JSON.stringify(a.body).slice(0, 300)}`);
  t.expect.equal(a.body.answer.applied, true);
  t.expect.equal(a.body.answer.choice, 'deny');

  await t.step('The agent was told no and says so');
  const answer = await t.fake.waitEvent('approval_answer');
  t.expect.ok(answer, 'the fake agent logged the answer');
  const done = firstRun(await m.wait(project, handle));
  t.expect.equal(done.status, 'completed');
  t.expect.includes(done.result, 'the command was refused');

  await t.step('The card is gone from the Inbox');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
});
