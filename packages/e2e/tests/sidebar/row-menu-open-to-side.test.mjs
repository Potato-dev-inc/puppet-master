// Right-click a pane row: "Open to the side" puts that pane next to the open one (split view).
import { calm, openPane } from './_area.mjs';

export const meta = {
  id: 'sidebar.row-menu-open-to-side',
  title: 'Right-click on a pane row offers Open to the side and splits the view',
  area: 'sidebar',
  tags: ['split'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });
  await openPane(t, 'Nimbus 1');

  await t.step('Right-click the second row');
  await t.page.rightClick('exact=Nimbus 2', { within: 'css=.ck-side' });
  await t.expect.visible('exact=Open to the side');

  await t.step('Choose Open to the side');
  await t.click('exact=Open to the side');
  await t.waitFor('css=.ck-split');
  await t.expect.count('css=.ck-split-side', 2);

  await t.step('Both chats are on screen, the first on the left');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-split-side[data-split-side=left] .ck-split-title')?.innerText`), 'Nimbus 1', 'left side');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-split-side[data-split-side=right] .ck-split-title')?.innerText`), 'Nimbus 2', 'right side');

  await t.step('Close split view: one pane again');
  await t.click('aria=Close split view');
  await t.expect.notVisible('css=.ck-split');
  await t.expect.text('NIMBUS 0.4.2');
}
