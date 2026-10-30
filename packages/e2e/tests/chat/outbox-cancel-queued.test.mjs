// Cancel on a waiting bubble takes the message back: the agent never gets it.
import { SLOW_AND_ECHO, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.outbox-cancel-queued',
  title: 'Cancel on a waiting message removes it before the agent sees it',
  area: 'chat',
  tags: ['outbox'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(SLOW_AND_ECHO);
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'slow job');
  await t.fake.waitEvent('prompt');

  await t.step('Queue a message and change your mind');
  await sendChat(t, 'regret this');
  await t.expect.text('The agent is working. Sends when the turn ends.');
  await t.click('exact=Cancel');

  await t.step('The bubble is gone');
  await t.expect.noText('regret this');
  await t.expect.noText('Sends when the turn ends');

  await t.step('A later message goes in and the cancelled one never does');
  await sendChat(t, 'keep this');
  await t.expect.text('ok this', { timeout: 30000 });
  const prompts = t.fake.events().filter((e) => e.ev === 'prompt').map((e) => e.text);
  t.expect.equal(prompts, ['slow job', 'keep this'], 'the cancelled message was never typed');
}
