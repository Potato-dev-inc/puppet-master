// At most five messages can wait in one pane; the sixth is refused with a plain note and stays in the box.
import { COMPOSER, SLOW_AND_ECHO, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.outbox-too-many-waiting',
  title: 'The sixth waiting message is refused with a note',
  area: 'chat',
  tags: ['outbox'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(SLOW_AND_ECHO.replace('ms = 9000', 'ms = 40000'));
  const pane = await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'slow job');
  await t.fake.waitEvent('prompt');

  await t.step('Fill the waiting list with five messages');
  // Typing five messages one by one takes long; the first four go in through the same call the Send button makes.
  for (const n of [1, 2, 3, 4]) {
    await t.page.eval(`window.__TAURI_INTERNALS__.invoke('pane_message_send', { paneId: ${JSON.stringify(pane.id)}, text: 'queued ${n}' })`);
  }
  await sendChat(t, 'queued 5');
  await t.expect.count('css=[data-testid=chat-pending]', 5);

  await t.step('The sixth is refused');
  await sendChat(t, 'one too many');
  await t.expect.text('Too many waiting messages.');
  await t.expect.value(COMPOSER, 'one too many');
  await t.expect.count('css=[data-testid=chat-pending]', 5);
}
