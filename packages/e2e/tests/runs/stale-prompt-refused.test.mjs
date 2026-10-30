// An answer for a prompt that is not the one on screen is refused (STALE_PROMPT); nothing is typed and the question stays.
import { mcp, firstRun, RUN_SCRIPT, guarded, waitBlocked } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.stale-prompt-refused',
  title: 'An answer with a wrong prompt id is refused and the question stays open',
  area: 'runs',
  tags: ['mcp', 'security'],
  edge: true,
};

export const run = guarded(async (t) => {
  t.fake.script(RUN_SCRIPT);
  const project = t.project('stale');
  const m = mcp(t);
  const handle = (await m.run({ agent_type: 'nimbus', project_path: project, task: 'ask first please' })).body.handle;
  const blocked = await waitBlocked(t, m, project, handle);
  t.expect.equal(blocked.status, 'needs_input');

  await t.step('Answer with an id that is not on screen');
  const bad = await m.answer({ project_path: project, handle, prompt_id: 'prompt-0000000000000000', choice: 'allow_once' });
  t.expect.equal(bad.status, 503);
  t.expect.equal(bad.body.code, 'STALE_PROMPT');

  await t.step('Nothing reached the agent and the card is still there');
  t.expect.ok(!t.fake.events().some((e) => e.ev === 'approval_answer'), 'the agent was not answered');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.visible('exact=Allow once');
  t.expect.equal(firstRun(await m.wait(project, handle, ['completed', 'failed', 'cancelled', 'needs_input'], 1000)).status, 'needs_input');

  await t.step('The right id still works');
  const ok = await m.answer({ project_path: project, handle, prompt_id: blocked.prompt.id, choice: 'allow_once' });
  t.expect.equal(ok.status, 200);
  t.expect.equal(firstRun(await m.wait(project, handle)).status, 'completed');
});
