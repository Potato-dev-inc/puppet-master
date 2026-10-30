// Edge case: the pane that is open is closed from outside (an MCP client). The cockpit does not stay on a dead pane.
import { calm, openPane, PANE_TITLE } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'sidebar.selected-pane-closed-outside',
  title: 'When the open pane is closed from outside the cockpit goes back to the Inbox',
  area: 'sidebar',
  tags: [],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const a = await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });
  await openPane(t, 'Nimbus 1');
  await t.expect.text('NIMBUS 0.4.2');

  await t.step('Close the open pane through the bridge');
  await a.kill();

  await t.step('The Inbox is shown and the other pane is still listed');
  await t.expect.notVisible(PANE_TITLE);
  await t.expect.text('Nothing needs you');
  await t.expect.count(PANE_ROW, 1);
  await t.expect.visible('exact=Nimbus', { within: 'css=.ck-side' });

  await t.step('The other pane opens normally');
  await openPane(t, 'Nimbus');
  await t.expect.text('NIMBUS 0.4.2');
}
