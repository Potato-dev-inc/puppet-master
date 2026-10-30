// Ctrl+\ puts the open pane next to its neighbour in the list (two chats side by side); Ctrl+\ again or the X closes it.
import { calm, openPane } from './_area.mjs';

export const meta = {
  id: 'panes.split-view-toggle',
  title: 'Ctrl+\\ shows two panes side by side and closes the split again',
  area: 'panes',
  tags: ['split', 'keyboard'],
};

const side = (t, which) => t.page.eval(`document.querySelector('.ck-split-side[data-split-side=${which}] .ck-split-title')?.innerText || ''`);

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });
  await openPane(t, 'Nimbus 2');

  await t.step('Press Ctrl+\\ with the second pane open');
  await t.press('Ctrl+\\');
  await t.waitFor('css=.ck-split');
  await t.expect.count('css=.ck-split-side', 2);

  await t.step('The open pane is on the left, its neighbour on the right');
  t.expect.equal(await side(t, 'left'), 'Nimbus 2');
  t.expect.equal(await side(t, 'right'), 'Nimbus 1');
  await t.expect.count('aria=Message to the agent', 2);

  await t.step('Ctrl+\\ again closes the split');
  await t.press('Ctrl+\\');
  await t.expect.notVisible('css=.ck-split');
  await t.expect.text('NIMBUS 0.4.2');

  await t.step('Open it again and close it with the X');
  await t.press('Ctrl+\\');
  await t.waitFor('css=.ck-split');
  await t.click('aria=Close split view');
  await t.expect.notVisible('css=.ck-split');
}
