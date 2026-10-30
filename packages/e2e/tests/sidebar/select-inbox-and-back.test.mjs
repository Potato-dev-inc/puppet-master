// A pane row opens the pane; the Inbox row goes back to the Inbox; the pane row stays where it was.
import { calm, PANE_TITLE, paneHeading } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'sidebar.select-pane-and-inbox',
  title: 'Clicking a pane opens it and clicking Inbox goes back to the Inbox',
  area: 'sidebar',
  tags: ['smoke'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');

  await t.step('Nothing is open: the Inbox shows');
  await t.expect.text('Nothing needs you');

  await t.step('Click the pane row');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor(PANE_TITLE);
  t.expect.equal(await paneHeading(t), 'Nimbus', 'the pane header names the agent');
  await t.expect.noText('Nothing needs you');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-row.is-active .ck-row-title')?.innerText`), 'Nimbus', 'the pane row is marked as the open one');

  await t.step('Click Inbox');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  await t.expect.notVisible(PANE_TITLE);
  await t.expect.count(PANE_ROW, 1);
}
