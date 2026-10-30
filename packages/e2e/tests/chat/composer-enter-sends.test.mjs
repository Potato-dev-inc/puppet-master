// Keyboard only: type in the box and press Enter. The agent gets the text and the box is empty again.
import { replyScript } from '../_helpers.mjs';
import { COMPOSER, openChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-enter-sends',
  title: 'Enter sends the message and empties the box',
  area: 'chat',
  tags: ['composer', 'smoke'],
};

export async function run(t) {
  t.fake.script(replyScript('got it'));
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Type a message and press Enter');
  await t.page.type('please do the thing', { into: COMPOSER });
  await t.press('Enter');

  await t.step('The agent got it and the box is empty');
  t.expect.equal((await t.fake.waitEvent('prompt')).text, 'please do the thing');
  await t.expect.value(COMPOSER, '');
  await t.expect.text('got it');
  await t.expect.text('please do the thing');
}
