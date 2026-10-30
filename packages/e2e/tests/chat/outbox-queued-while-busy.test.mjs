// A message written while the agent is working waits as a bubble ("Sends when the turn ends") and goes in afterwards.
import { SLOW_AND_ECHO, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.outbox-queued-while-busy',
  title: 'A message written while the agent works waits, then goes in when the turn ends',
  area: 'chat',
  tags: ['outbox', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(SLOW_AND_ECHO);
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Start a slow job');
  await sendChat(t, 'slow job');
  await t.fake.waitEvent('prompt');

  await t.step('Write a second message while the agent works');
  await sendChat(t, 'second one');

  await t.step('It waits as a bubble and says why');
  await t.expect.text('second one');
  await t.expect.text('The agent is working. Sends when the turn ends.');
  await t.expect.visible('exact=Cancel');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 1, 'the agent has only the first message so far');

  await t.step('When the turn ends the waiting message goes in');
  await t.expect.text('slow job done');
  const second = await t.fake.waitEvent((e) => e.ev === 'prompt' && e.text === 'second one', { timeout: 30000 });
  t.expect.equal(second.text, 'second one');
  await t.expect.text('ok one');
  await t.expect.notVisible('exact=Cancel');
}
