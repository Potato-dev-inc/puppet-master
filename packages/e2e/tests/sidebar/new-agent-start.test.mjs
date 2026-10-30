// Picking an agent in the + menu starts a pane of it; it shows in the sidebar and opens.
import { calm, PANE_TITLE, paneHeading } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'sidebar.new-agent-start',
  title: 'Picking Nimbus in the + menu starts a pane that opens in the cockpit',
  area: 'sidebar',
  tags: ['smoke', 'spawn'],
};

export async function run(t) {
  await calm(t);

  await t.step('Open the + menu and pick Nimbus');
  await t.click('aria=New agent');
  await t.click('exact=Nimbus', { within: 'css=.ck-menu' });

  await t.step('The menu closes and the agent starts');
  await t.expect.notVisible('css=.ck-menu');
  await t.expect.count(PANE_ROW, 1);
  await t.waitFor(PANE_TITLE);
  t.expect.equal(await paneHeading(t), 'Nimbus', 'the new pane is open');
  await t.expect.text('NIMBUS 0.4.2');

  const panes = await t.panes();
  t.expect.equal(panes.length, 1, 'the app lists one pane');
  t.expect.equal(panes[0].agent_type, 'nimbus', 'it is a Nimbus pane');

  await t.step('A second one gets numbered names');
  await t.click('aria=New agent');
  await t.click('exact=Nimbus', { within: 'css=.ck-menu' });
  await t.expect.count(PANE_ROW, 2);
  await t.expect.visible('exact=Nimbus 1', { within: 'css=.ck-side' });
  await t.expect.visible('exact=Nimbus 2', { within: 'css=.ck-side' });
}
