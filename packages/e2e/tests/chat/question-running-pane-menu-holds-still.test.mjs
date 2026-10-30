// The safety net: the pane still says "running" but its screen holds a menu that has not moved for about 3 seconds.
import { installSlowIdleAgent, turnScript, think, hold, keysSeen, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.question-running-pane-menu-holds-still',
  title: 'A menu that holds still on a pane that still says running becomes a card after about 3 seconds',
  area: 'chat',
  tags: ['questions', 'safety-net'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  // This agent takes 5 seconds of a still screen before it counts as idle, so "running" lasts long enough to see.
  await installSlowIdleAgent(t);
  t.fake.script(turnScript([think(300), hold('Which flavour?\n> 1. Alpha\n  2. Beta\nctrl+c to stop')]));
  const pane = await t.spawnPane('nimbus5');
  await openChat(t);
  await sendChat(t, 'go');

  await t.step('The menu is drawn; the pane still reads as running');
  await pane.waitScreen('2. Beta');
  const drawnAt = Date.now();
  t.expect.equal((await pane.info()).status, 'running', 'the pane says running');
  await t.expect.notVisible('text=Agent is asking');

  await t.step('Not at once: the card waits until the screen has held still');
  await t.expect.text('Agent is asking', { timeout: 20000 });
  const waited = Date.now() - drawnAt;
  t.log(`card after ${waited} ms`);
  t.expect.ok(waited >= 2000, `the card came after the screen held still (${waited} ms)`);
  t.expect.equal((await pane.info()).status, 'running', 'the pane still said running when the card came');
  await t.expect.visible('exact=Alpha');
  await t.expect.visible('exact=Beta');

  await t.step('Answering it works as for any card');
  await t.click('exact=Beta');
  await t.fake.waitEvent('turn_done');
  t.expect.equal(keysSeen(t).join(','), 'Enter,Down,Enter', 'the cursor moved to the second row and Enter was pressed');
}
