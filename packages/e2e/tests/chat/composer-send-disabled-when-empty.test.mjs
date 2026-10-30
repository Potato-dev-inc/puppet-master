// Send stays off until there is something to send: an empty box and a box with only spaces send nothing.
import { COMPOSER, openChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-send-disabled-when-empty',
  title: 'Send is off while the message box is empty or only spaces',
  area: 'chat',
  tags: ['composer'],
  edge: true,
};

const sendOff = (t) => t.page.eval(`document.querySelector('.ck-chat-send').disabled`);

export async function run(t) {
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('The empty box cannot send');
  await t.expect.eventually(() => sendOff(t), { message: 'Send should be off in an empty box' });

  await t.step('Only spaces still cannot send');
  await t.page.type('     ', { into: COMPOSER });
  await t.expect.eventually(() => sendOff(t), { message: 'Send should be off for spaces only' });

  await t.step('Enter on an empty box sends nothing');
  await t.press('Enter');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'nothing reached the agent');

  await t.step('A real word turns Send on');
  await t.page.type('hello');
  await t.expect.eventually(async () => !(await sendOff(t)), { message: 'Send should turn on' });

  await t.step('Deleting it turns Send off again');
  await t.page.fill(COMPOSER, '');
  await t.expect.eventually(() => sendOff(t), { message: 'Send should be off again' });
}
