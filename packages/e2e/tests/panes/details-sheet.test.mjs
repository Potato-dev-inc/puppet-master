// Pane menu > Details shows the folder, the screen size and the process id of the pane, and Done closes it.
import { calm, openPane } from './_area.mjs';

export const meta = {
  id: 'panes.details-sheet',
  title: 'Details in the pane menu shows the folder, size and process of the pane',
  area: 'panes',
  tags: ['details'],
};

export async function run(t) {
  await calm(t);
  const folder = t.project('details-folder');
  const pane = await t.spawnPane('nimbus', { cwd: folder });
  await openPane(t, 'Nimbus');

  await t.step('Pane menu > Details');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Details');
  await t.waitFor('css=.ck-facts--details');

  await t.step('The facts match what the app knows');
  const info = await pane.info();
  const facts = await t.page.eval(`Object.fromEntries([...document.querySelectorAll('.ck-facts--details .ck-facts-row')].map((r) => [r.querySelector('dt').innerText.trim(), r.querySelector('dd').innerText.trim()]))`);
  t.expect.equal(facts.Folder, folder, 'the folder');
  t.expect.equal(facts['Process ID'], String(info.pid), 'the process id');
  t.expect.match(facts['Screen size'], /^\d+×\d+$/, 'the screen size reads columns × rows');

  await t.step('Done closes the sheet');
  await t.click('exact=Done');
  await t.expect.notVisible('css=.ck-facts--details');
  await t.expect.text('NIMBUS 0.4.2');
}
