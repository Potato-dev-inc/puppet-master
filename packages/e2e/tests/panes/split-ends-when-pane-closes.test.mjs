// Edge case: the pane on the right of a split is closed from outside. The split ends by itself and the left pane stays open.
import { calm, openPane } from './_area.mjs';

export const meta = {
  id: 'panes.split-ends-when-pane-closes',
  title: 'Closing the pane on the right ends the split and keeps the left pane',
  area: 'panes',
  tags: ['split'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const first = await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });
  await openPane(t, 'Nimbus 2');
  await t.press('Ctrl+\\');
  await t.waitFor('css=.ck-split');

  await t.step('Close the right-hand pane (Nimbus 1) from outside');
  await first.kill();

  await t.step('The split is gone and the left pane is open on its own');
  await t.expect.notVisible('css=.ck-split');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('.ck-pane-head .ck-title')?.innerText`)) === 'Nimbus', { message: 'the left pane is open (now the only Nimbus)' });
  await t.expect.text('NIMBUS 0.4.2');
}
