// Output stress: one reply of 2000 rows (far taller than the pane). The newest rows are on screen and the app stays usable.
import { calm, S, oneTurn, openPane, waitStatus } from './_area.mjs';

export const meta = {
  id: 'work.output-two-thousand-lines',
  title: 'A reply of 2000 rows ends at row 2000 in the terminal and the cockpit stays usable',
  area: 'work',
  tags: ['work', 'output'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(300), S.lines(2000, 'output row {n}'), S.say('all rows printed')]));
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await t.click('exact=Terminal');

  await t.step('Ask for the long reply');
  await pane.write('print everything');
  await t.fake.waitEvent('prompt');
  await waitStatus(t, pane, 'idle', 60000);

  await t.step('The terminal ends at the last row');
  await t.expect.text('all rows printed', { timeout: 30000 });
  const screen = await pane.screen();
  t.expect.includes(screen, 'output row 2000');
  t.expect.includes(screen, 'all rows printed');

  await t.step('Other tabs still answer');
  await t.click('exact=History');
  await t.expect.eventually(async () => /No checkpoints yet|Restoring puts your files back/.test(await t.page.text()), { message: 'the History tab opens' });
  await t.click('exact=Terminal');
  await t.expect.text('all rows printed');
}
