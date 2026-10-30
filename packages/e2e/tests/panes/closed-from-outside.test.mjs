// Edge case: a pane is closed through the bridge (as an MCP client would) while the cockpit shows it.
import { PANE_ROW, selectPane } from '../_helpers.mjs';

import { calm } from './_area.mjs';

export const meta = {
  id: 'panes.closed-from-outside',
  title: 'A pane closed through the bridge disappears from the sidebar',
  area: 'panes',
  tags: [],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const pane = await t.spawnPane('nimbus');
  await t.expect.count(PANE_ROW, 1);
  await selectPane(t);
  await t.expect.text('NIMBUS 0.4.2');

  await t.step('Close the pane from outside the UI');
  await pane.kill();

  await t.step('The sidebar is empty again');
  await t.expect.count(PANE_ROW, 0);
  await t.expect.text('No agents running.');
  t.expect.equal((await t.panes()).length, 0, 'the app lists no panes');
}
