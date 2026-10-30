// A yes/no question that no profile reads gets Yes and No buttons.
import { installAsker, turnScript, think, hold, keysSeen, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-yes-no-buttons',
  title: 'A yes/no question shows Yes and No and Yes answers it',
  area: 'chat',
  tags: ['questions'],
};

export async function run(t) {
  await installAsker(t);
  t.fake.script(turnScript([think(300), hold('Continue with the install yes/no please answer')]));
  await t.spawnPane('asker');
  await openChat(t);
  await sendChat(t, 'go');

  await t.step('The card offers Yes and No');
  await t.expect.text('Agent is asking');
  await t.expect.visible('exact=Yes');
  await t.expect.visible('exact=No');

  await t.step('Click Yes');
  await t.click('exact=Yes');

  await t.step('The agent got y and Enter and went on');
  await t.fake.waitEvent('turn_done');
  await t.expect.eventually(() => keysSeen(t).join(',') === 'Enter,Enter', { message: () => `the agent saw: ${keysSeen(t).join(',')}` });
  await t.expect.notVisible('exact=Yes');
}
