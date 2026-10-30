// With no agent the sidebar says so, offers the companion and the Inbox is the open page.
import { calm, sidebarGroups } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'sidebar.empty-state',
  title: 'With no agents the sidebar says "No agents running." and the Inbox is open',
  area: 'sidebar',
  tags: ['smoke', 'empty'],
  edge: true,
};

export async function run(t) {
  await calm(t);

  await t.step('The sidebar has no groups and says nothing runs');
  await t.expect.text('No agents running.');
  await t.expect.count(PANE_ROW, 0);
  t.expect.equal((await sidebarGroups(t)).length, 0, 'no group captions');

  await t.step('The companion and the Inbox are there');
  await t.expect.visible('Make a companion');
  await t.expect.visible('exact=Inbox', { within: 'css=.ck-side' });
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-side .ck-row--flat')?.classList.contains('is-active')`), true, 'the Inbox row is the active one');

  await t.step('The main area is the empty Inbox');
  await t.expect.text('Nothing needs you');
  await t.expect.text('Agents that stop to ask a question will show up here.');
}
