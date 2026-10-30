// A working agent redraws (a spinner, a timer). A menu in its output is work in progress, not a question: no card.
import { turnScript, think, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-running-screen-keeps-changing',
  title: 'A menu in the output of an agent that keeps working gets no card',
  area: 'chat',
  tags: ['questions', 'safety-net'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(turnScript(['{ do = "screen", text = "Which flavour?\\n> 1. Alpha\\n  2. Beta" }', think(14000)]));
  const pane = await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'go');

  await t.step('The menu is on the screen and the agent is still working');
  await pane.waitScreen('2. Beta');
  await t.expect.eventually(async () => (await pane.info()).status === 'running', { message: 'the pane is running' });

  await t.step('Nothing asks for an answer while the screen keeps changing');
  const end = Date.now() + 7000;
  while (Date.now() < end) {
    t.expect.ok(!(await t.page.text()).includes('Agent is asking'), 'a card showed up for a working agent');
    t.expect.equal((await pane.info()).status, 'running', 'the agent is still working');
    await t.sleep(400);
  }
}
