// A question that has no menu and no yes/no: the card shows what the agent says and a box to type the answer.
import { installAsker, turnScript, think, hold, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-free-text-answer',
  title: 'A free-text question gets a box and the typed answer reaches the agent',
  area: 'chat',
  tags: ['questions'],
};

export async function run(t) {
  await installAsker(t);
  t.fake.script(turnScript([think(300), hold('What should the project be called? please answer')]));
  await t.spawnPane('asker');
  await openChat(t);
  await sendChat(t, 'go');

  await t.step('The card shows the question and a box');
  await t.expect.text('Agent is asking');
  await t.expect.text('What should the project be called?');
  await t.expect.visible('aria=Type an answer');
  await t.expect.notVisible('exact=Yes');

  await t.step('Send is off until something is typed');
  t.expect.ok(await t.page.eval(`document.querySelector('.ib-screen-answer button[type=submit]').disabled`), 'Send is off for an empty answer');

  await t.step('Type an answer and send it');
  // The fake agent ends its question on the very first key it gets, so the dash is taken by the question and the rest
  // is typed into its input box and sent: the agent then receives " Atlas".
  await t.page.type('- Atlas', { into: 'aria=Type an answer' });
  await t.click('css=.ib-screen-answer button[type=submit]');

  await t.step('The agent got the answer and the box is empty again');
  const prompt = await t.fake.waitEvent((e) => e.ev === 'prompt' && e.text !== 'go');
  t.expect.equal(prompt.text.trim(), 'Atlas');
  await t.expect.value('aria=Type an answer', '');
}
