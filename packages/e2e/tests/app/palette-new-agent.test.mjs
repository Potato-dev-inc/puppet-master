// "New Nimbus" in the palette starts an agent pane; it appears in the sidebar and is the open pane.
import { openPalette, PANE_TITLE, calm } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'app.palette-new-agent',
  title: 'New Nimbus in the palette starts an agent pane',
  area: 'app',
  tags: ['palette', 'spawn'],
};

export async function run(t) {
  await calm(t);
  await t.expect.count(PANE_ROW, 0);

  await t.step('Ctrl+K, type "new nimbus", Enter');
  await openPalette(t);
  await t.type('new nimbus');
  await t.expect.text('New Nimbus');
  await t.press('Enter');

  await t.step('A pane starts and opens');
  await t.expect.count(PANE_ROW, 1);
  await t.waitFor(PANE_TITLE);
  await t.expect.text('NIMBUS 0.4.2');

  const panes = await t.panes();
  t.expect.equal(panes.length, 1, 'the app lists one pane');
  t.expect.equal(panes[0].agent_type, 'nimbus', 'it is a Nimbus pane');
}
