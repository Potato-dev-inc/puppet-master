// Several messages written during one turn go in one at a time, in the order they were written.
import { SLOW_AND_ECHO, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.outbox-several-in-order',
  title: 'Three waiting messages are delivered one by one, in order',
  area: 'chat',
  tags: ['outbox'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  t.fake.script(SLOW_AND_ECHO.replace('ms = 9000', 'ms = 25000'));
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'slow job');
  await t.fake.waitEvent('prompt');

  await t.step('Write three messages while the agent works');
  await sendChat(t, 'message one');
  await sendChat(t, 'message two');
  await sendChat(t, 'message three');

  await t.step('The first waits for the turn, the others wait for the ones before them');
  await t.expect.text('The agent is working. Sends when the turn ends.');
  await t.expect.text('Waits for the messages before it.');
  await t.expect.count('css=[data-testid=chat-pending]', 3);

  await t.step('They arrive in order');
  await t.expect.text('ok three', { timeout: 60000 });
  const prompts = t.fake.events().filter((e) => e.ev === 'prompt').map((e) => e.text);
  t.expect.equal(prompts, ['slow job', 'message one', 'message two', 'message three']);
}
