// More than 8000 characters cannot be sent: the box says so and Send is off. Near the limit it counts.
import { COMPOSER, openChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-too-long-blocked',
  title: 'A message over 8000 characters is refused with a plain note',
  area: 'chat',
  tags: ['composer'],
  edge: true,
};

const sendOff = (t) => t.page.eval(`document.querySelector('.ck-chat-send').disabled`);

export async function run(t) {
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Paste 7500 characters: the count shows');
  await t.page.type('a'.repeat(7500), { into: COMPOSER, delay: 0 });
  await t.expect.text('7500 / 8000');
  t.expect.ok(!(await sendOff(t)), 'Send is still on');

  await t.step('Go over the limit: a note appears and Send is off');
  await t.page.type('b'.repeat(501), { delay: 0 });
  await t.expect.text('The message is too long. The most is 8000 characters.');
  await t.expect.eventually(() => sendOff(t), { message: 'Send should be off over the limit' });
  await t.press('Enter');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'nothing was sent');

  await t.step('Cut it back: the note goes away');
  await t.page.fill(COMPOSER, 'short again', { delay: 0 });
  await t.expect.noText('The message is too long');
  await t.expect.eventually(async () => !(await sendOff(t)), { message: 'Send should be on again' });
}
