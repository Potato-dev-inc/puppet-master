// Clip It: an agent whose chat is only read from its screen has no turn end line, so no clip button.
import { openChat, sendChat, replyScript } from '../_helpers.mjs';

export const meta = {
  id: 'clip.screen-chat-has-no-clip',
  title: 'A chat read from the screen has no Make clip button',
  area: 'clip',
  tags: ['clip'],
  edge: true,
};

export async function run(t) {
  t.fake.script(replyScript('all done', 400));
  await t.spawnPane('nimbus');

  await t.step('Chat with an agent that has no transcript file');
  await openChat(t);
  await sendChat(t, 'say something');
  await t.expect.text('all done');
  await t.expect.text('Read from the screen');

  await t.step('There is no end line with Make clip or Branch');
  await t.expect.notVisible('exact=Make clip');
  await t.expect.notVisible('exact=Branch');
}
