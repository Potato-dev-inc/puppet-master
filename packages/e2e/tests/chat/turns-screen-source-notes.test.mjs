// An agent whose chat is read off the terminal says so, and says the text may be cut. Before any message the chat waits.
import { replyScript } from '../_helpers.mjs';
import { openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.turns-screen-source-notes',
  title: 'A chat read from the terminal says that it is a reading of the screen',
  area: 'chat',
  tags: ['turns'],
  edge: true,
};

export async function run(t) {
  t.fake.script(replyScript('screen reply'));
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Before any message: the empty chat waits and the header has no model');
  await t.expect.text('Waiting for a message on the terminal');
  await t.expect.text('Model not known yet');
  await t.expect.text('Messages wait until the agent is free.');
  await t.expect.text('Read from the screen: text may be cut or wrapped.');

  await t.step('After a message the reply is shown as the screen has it');
  await sendChat(t, 'say something');
  await t.expect.text('reply: screen reply');
  await t.expect.noText('Waiting for a message on the terminal');
  await t.expect.text('Read from the screen: text may be cut or wrapped.');
}
