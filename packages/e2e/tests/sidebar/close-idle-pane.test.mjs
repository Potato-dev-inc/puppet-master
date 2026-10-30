// Pane menu > Close on an idle agent closes it at once, no question asked.
import { calm, PANE_TITLE } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'sidebar.close-idle-pane',
  title: 'Closing an idle pane removes it at once and returns to the Inbox',
  area: 'sidebar',
  tags: ['smoke', 'close'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor(PANE_TITLE);

  await t.step('Pane menu > Close');
  await t.click('aria=Pane actions');
  await t.click('exact=Close');

  await t.step('The pane is gone and nothing asked first');
  await t.expect.count(PANE_ROW, 0);
  await t.expect.text('No agents running.');
  await t.expect.text('Nothing needs you');
  await t.expect.noText('Keep running');
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'the app lists no pane' });
}
