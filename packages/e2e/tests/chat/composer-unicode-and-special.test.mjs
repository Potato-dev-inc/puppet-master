// Letters from other languages, an emoji, quotes, a backslash and angle brackets all arrive as typed.
import { turnScript, think, say, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-unicode-and-special',
  title: 'Unicode, an emoji and special characters reach the agent unchanged',
  area: 'chat',
  tags: ['composer'],
  edge: true,
};

const TEXT = 'Grüße café 日本語 😀 "quoted" it\'s C:\\dir <b>x</b> $HOME 100%';

export async function run(t) {
  t.fake.script(turnScript([think(200), say('fine')]));
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Send a message with unusual characters');
  await sendChat(t, TEXT);

  await t.step('The bubble and the agent both have the same text');
  await t.expect.text(TEXT);
  t.expect.equal((await t.fake.waitEvent('prompt')).text, TEXT, 'the agent received the exact text');
  await t.expect.text('fine');
}
