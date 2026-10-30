// Edge case: several panes of one agent are numbered; when one closes the others are renumbered (one left: no number).
import { calm, sidebarTitles } from './_area.mjs';

export const meta = {
  id: 'sidebar.numbering-follows-panes',
  title: 'Panes of one agent are numbered and the numbers follow when one closes',
  area: 'sidebar',
  tags: [],
  edge: true,
};

export async function run(t) {
  await calm(t);

  await t.step('Start three agents of the same kind');
  const a = await t.spawnPane('nimbus');
  const b = await t.spawnPane('nimbus');
  await t.spawnPane('nimbus');
  await t.expect.eventually(async () => (await sidebarTitles(t)).length === 3, { message: 'three rows' });
  t.expect.equal((await sidebarTitles(t)).sort(), ['Nimbus 1', 'Nimbus 2', 'Nimbus 3'], 'numbered 1 to 3');

  await t.step('Close the first one');
  await a.kill();
  await t.expect.eventually(async () => (await sidebarTitles(t)).length === 2, { message: 'two rows left' });
  t.expect.equal((await sidebarTitles(t)).sort(), ['Nimbus 1', 'Nimbus 2'], 'the two left are numbered 1 and 2');

  await t.step('Close another: the last one has no number');
  await b.kill();
  await t.expect.eventually(async () => (await sidebarTitles(t)).join() === 'Nimbus', { message: 'one row, just "Nimbus"' });
}
