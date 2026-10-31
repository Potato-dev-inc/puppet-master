// The alert is about two live agents. When one of them is closed the alert goes away by itself.
import { calm } from './_area.mjs';
import { twoWriters, ALERT_TITLE } from '../collision/_area.mjs';

export const meta = {
  id: 'work.collision-closing-one-clears-alert',
  title: 'Closing one of the two colliding agents makes the alert go away',
  area: 'work',
  tags: ['work', 'collision'],
  edge: true,
  timeout: 180000,
};

export async function run(t) {
  await calm(t);
  const { a, b } = await twoWriters(t);
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.step('The alert is up');
  await t.expect.text(ALERT_TITLE, { timeout: 90000 });

  await t.step('One agent is closed from outside the app (the bridge)');
  await b.kill();
  await t.expect.eventually(async () => (await t.panes()).length === 1, { message: 'one pane left' });

  await t.step('The Inbox has nothing about a collision any more');
  await t.expect.noText(ALERT_TITLE, { timeout: 60000 });
  t.expect.equal((await a.info()).status, 'running', 'the other agent keeps working');
}
