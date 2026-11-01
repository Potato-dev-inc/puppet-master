// Chat (ACP) on the agent tools: an orchestrating agent starts a worker as a chat session with run_agent(lane: "acp"),
// waits for it, reads its question (not a screen), and can answer it only when the person flipped the switch for that
// chat. Another agent may read the chat but not steer it. The agent is the fake program with --acp.
import path from 'node:path';
import { chatPanes, MESSAGE_BOX, calm } from './_area.mjs';
import { mcp, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'acp.agent-tools',
  title: 'run_agent(lane: acp): wait, inspect and answer_prompt only when the person allowed it',
  area: 'acp',
  tags: ['acp', 'mcp', 'security'],
  seed: { acp_chat_enabled: true },
  timeout: 120000,
};

const shot = (t, name) => t.page.screenshot(path.join(t.outDir, `${name}.png`));

export const run = guarded(async (t) => {
  await calm(t);
  const project = t.project('acp-tools');
  const boss = mcp(t, '7e57c0de-0000-4000-8000-0000000000b1');
  const other = mcp(t, '7e57c0de-0000-4000-8000-0000000000b2');

  await t.step('An orchestrating agent starts a chat worker with lane: acp');
  const started = await boss.call('POST', '/agents/run', { agent_type: 'nimbus', project_path: project, task: 'perm', lane: 'acp', background: true });
  t.expect.equal(started.status, 200, JSON.stringify(started.body).slice(0, 300));
  const handle = started.body.handle;
  t.expect.equal(started.body.lane, 'acp');
  t.expect.equal((await chatPanes(t)).map((p) => p.id), [handle], 'the handle is the chat pane');

  await t.step('wait_agents reports that it needs an answer, with the options the agent offered');
  const waited = await boss.wait(project, [handle]);
  const view = waited.body.agents[0];
  t.expect.equal(view.status, 'waiting_input');
  t.expect.equal(view.prompt.options.map((o) => o.id).sort(), ['allow_always', 'allow_once', 'reject_once']);
  t.expect.equal(view.prompt.answerable_by_agent, false, 'the person has not let agents answer');

  await t.step('answer_prompt is refused: only the person can answer');
  const refused = await boss.answer({ handle, prompt_id: view.prompt.prompt_id, choice: 'allow_once' });
  t.expect.equal(refused.body.code, 'AGENT_ANSWER_NOT_ALLOWED');
  t.expect.equal((await boss.get(`/agents/${handle}`)).body.status, 'waiting_input', 'still waiting');

  await t.step('The person sees the card and flips the switch for this chat');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor('css=[data-testid=acp-dock]');
  await t.expect.text('Allow Once');
  await t.expect.text('Off: only you can answer.');
  await shot(t, '1-switch-off');
  await t.click('css=[data-testid=agent-answering] [role=switch]');
  await t.expect.text('On: an agent that started this chat may answer');
  await shot(t, '2-switch-on');

  await t.step('Another agent may read the chat but not answer or steer it');
  const read = await other.get(`/agents/${handle}`);
  t.expect.equal(read.status, 200);
  t.expect.equal(read.body.capabilities.terminal_screen, false);
  t.expect.equal((await other.answer({ handle, prompt_id: view.prompt.prompt_id, choice: 'allow_once' })).body.code, 'NOT_YOUR_CHAT');
  t.expect.equal((await other.followup(project, handle, 'hello')).body.code, 'NOT_YOUR_CHAT');

  await t.step('The agent that started it answers a choice the agent offered, and not any other');
  t.expect.equal((await boss.answer({ handle, prompt_id: view.prompt.prompt_id, choice: 'yes please' })).body.code, 'INVALID_CHOICE');
  const answered = await boss.answer({ handle, prompt_id: view.prompt.prompt_id, choice: 'reject_once' });
  t.expect.equal(answered.status, 200, JSON.stringify(answered.body).slice(0, 300));
  const done = await boss.wait(project, [handle], ['completed', 'failed', 'cancelled'], 20000);
  t.expect.equal(done.body.agents[0].status, 'completed');
  t.expect.equal(done.body.agents[0].result, 'chose:reject_once');
  await t.waitFor(MESSAGE_BOX);
  await t.expect.text('chose:reject_once');
  await shot(t, '3-answered');

  await t.step('followup_task gives the same chat its next turn; inspect shows the conversation');
  const next = await boss.followup(project, handle, 'stream 3', { background: false, wait_ms: 20000 });
  t.expect.equal(next.body.status, 'completed', JSON.stringify(next.body).slice(0, 300));
  t.expect.equal(next.body.result, 'w0 w1 w2 ');
  const inspected = (await boss.get(`/agents/${handle}`)).body;
  t.expect.ok(inspected.recent.some((r) => r.kind === 'user' && r.text === 'stream 3'), 'the chat is in inspect_agent');

  await t.step('close_agent closes the chat pane');
  const closed = await boss.close(project, handle);
  t.expect.equal(closed.status, 200, JSON.stringify(closed.body).slice(0, 300));
  await t.expect.eventually(async () => (await chatPanes(t)).length === 0, { message: 'the chat pane is gone' });
});
