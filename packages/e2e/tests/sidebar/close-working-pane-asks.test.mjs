// Edge case: closing an agent that is working asks first. Keep running cancels; Close agent ends it.
import { calm, PANE_TITLE, slowScript } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'sidebar.close-working-pane-asks',
  title: 'Closing a working pane asks first; Keep running cancels and Close agent ends it',
  area: 'sidebar',
  tags: ['close'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  t.fake.script(slowScript(60000));
  const pane = await t.spawnPane('nimbus');
  await pane.write('longtask');
  await t.click('exact=longtask', { within: 'css=.ck-side' });
  await t.waitFor(PANE_TITLE);
  await t.expect.text('Working');

  await t.step('Pane menu > Close asks first');
  await t.click('aria=Pane actions');
  await t.click('exact=Close');
  await t.expect.text('Close Nimbus?');
  await t.expect.text('It is still running.');

  await t.step('Keep running: nothing closed');
  await t.click('exact=Keep running');
  await t.expect.notVisible('exact=Keep running');
  await t.expect.count(PANE_ROW, 1);
  t.expect.equal((await t.panes()).length, 1, 'the pane still runs');

  await t.step('Close again and confirm');
  await t.click('aria=Pane actions');
  await t.click('exact=Close');
  await t.click('exact=Close agent');
  await t.expect.count(PANE_ROW, 0);
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'the app lists no pane' });
}
