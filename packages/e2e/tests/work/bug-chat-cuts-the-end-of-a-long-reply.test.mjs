// Was a BUG (fixed): for an agent whose conversation Chat reads from the screen, a reply of 1500 rows is cut at about row 1318 (mid-word:
// "output ro"). The OLDEST part is kept and the NEWEST part is dropped, so the agent's closing words (and a question it asks
// at the end) never show in Chat, even when the agent has been idle for a while. A reply of 1000 rows is complete.
import { calm, S, oneTurn, openChat, sendChat, waitStatus } from './_area.mjs';
import { knownBug } from './_area.mjs';

export const meta = {
  id: 'work.bug-chat-cuts-the-end-of-a-long-reply',
  title: 'Chat shows the end of a 1500-row reply ("all rows printed"), not only its first 1300 rows',
  area: 'work',
  tags: ['work', 'output'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(300), S.lines(1500, 'output row {n}'), S.say('all rows printed')]));
  const pane = await t.spawnPane('nimbus');
  await openChat(t);
  await t.step('Ask for a reply of 1500 rows');
  await sendChat(t, 'print 1500 rows');
  await t.fake.waitEvent('turn_done', { timeout: 60000 });
  await waitStatus(t, pane, 'idle', 60000);
  t.expect.includes(await pane.screen(), 'all rows printed');

  await t.step('Chat should end with the agent\'s last line');
  await t.expect.text('all rows printed', { timeout: 15000 });
}
