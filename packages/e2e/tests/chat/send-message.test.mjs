// Send a message from the Chat tab: the fake agent receives it and the reply shows in the chat.
import { openChat, sendChat, replyScript } from '../_helpers.mjs';

export const meta = {
  id: 'chat.send-message',
  title: 'A message sent from Chat reaches the agent and the reply comes back',
  area: 'chat',
  tags: ['smoke'],
};

export async function run(t) {
  t.fake.script(replyScript('pong from the fake agent'));
  await t.spawnPane('nimbus');

  await t.step('Open the Chat tab');
  await openChat(t);
  await t.expect.text('Messages wait until the agent is free.');

  await t.step('Send a message');
  await sendChat(t, 'hello fake agent');

  await t.step('The agent got exactly that text');
  const prompt = await t.fake.waitEvent('prompt');
  t.expect.equal(prompt.text, 'hello fake agent', 'the agent received the message');

  await t.step('The reply shows in the chat');
  await t.expect.text('pong from the fake agent');
  await t.expect.text('Sent');
}
