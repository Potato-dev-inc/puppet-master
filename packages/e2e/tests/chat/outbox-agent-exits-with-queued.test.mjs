// The agent quits while a message waits: the message is not lost silently. It shows a failure with Copy text and Hide.
import { turnScript, think, say, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.outbox-agent-exits-with-queued',
  title: 'A waiting message fails in plain words when the agent exits',
  area: 'chat',
  tags: ['outbox', 'errors'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(turnScript([think(3000), say('bye'), '{ do = "exit", code = 3 }'], { on_prompt: 'quit soon' }));
  const pane = await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'quit soon');
  await t.fake.waitEvent('prompt');

  await t.step('Queue a message while the agent is still working');
  await sendChat(t, 'for nobody');
  await t.expect.text('The agent is working. Sends when the turn ends.');

  await t.step('The agent exits');
  await t.expect.text('The agent is not running.');

  await t.step('The waiting message says it was not sent');
  await t.expect.text('The agent is not running in this pane.');
  await t.expect.visible('exact=Copy text');
  await t.expect.visible('exact=Hide');
  t.expect.equal((await pane.info()).status, 'error');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 1, 'the second message never reached the agent');

  await t.step('The message box is switched off');
  t.expect.ok(await t.page.eval(`document.querySelector('textarea[aria-label="Message to the agent"]').disabled`), 'the box is disabled');
}
