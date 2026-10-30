// Edge case: Escape while naming a pane throws the change away.
import { calm, PANE_TITLE, paneHeading } from './_area.mjs';

export const meta = {
  id: 'sidebar.rename-escape-cancels',
  title: 'Esc while renaming keeps the old name',
  area: 'sidebar',
  tags: ['rename', 'keyboard'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor(PANE_TITLE);

  await t.step('Double-click the title and type a name');
  await t.page.doubleClick(PANE_TITLE);
  await t.waitFor('aria=Pane name');
  await t.page.fill('aria=Pane name', 'Throw this away');

  await t.step('Press Escape');
  await t.press('Escape');
  await t.expect.notVisible('aria=Pane name');
  t.expect.equal(await paneHeading(t), 'Nimbus', 'the name did not change');
  await t.expect.noText('Throw this away');
}
