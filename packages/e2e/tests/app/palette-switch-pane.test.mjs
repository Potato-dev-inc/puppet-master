// The palette lists every pane; picking one with Enter opens it.
import { openPalette, paneHeading, paletteTitles, calm } from './_area.mjs';

export const meta = {
  id: 'app.palette-switch-pane',
  title: 'The palette lists the panes and Enter on one opens it',
  area: 'app',
  tags: ['palette', 'keyboard'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });

  await t.step('Ctrl+K: both panes are in the list, under Panes');
  await openPalette(t);
  await t.expect.text('Panes');
  const titles = await paletteTitles(t);
  t.expect.includes(titles, 'Nimbus 1');
  t.expect.includes(titles, 'Nimbus 2');

  await t.step('Type "Nimbus 2" and press Enter');
  await t.type('Nimbus 2');
  await t.press('Enter');

  await t.step('The second pane is open');
  await t.expect.notVisible('aria=Search commands');
  await t.expect.eventually(async () => (await paneHeading(t)) === 'Nimbus 2', { message: 'the header says Nimbus 2' });
  await t.expect.text('NIMBUS 0.4.2');

  await t.step('Again with the first pane');
  await openPalette(t);
  await t.type('Nimbus 1');
  await t.press('Enter');
  await t.expect.eventually(async () => (await paneHeading(t)) === 'Nimbus 1', { message: 'the header says Nimbus 1' });
}
