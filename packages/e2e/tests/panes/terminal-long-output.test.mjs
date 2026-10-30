// Edge case: an answer far taller than the pane. The newest rows are on screen and the app stays responsive.
import { calm, openPane, focusTerminal } from './_area.mjs';

export const meta = {
  id: 'panes.terminal-long-output',
  title: 'A reply of 300 rows ends at the last row in the terminal',
  area: 'panes',
  tags: ['terminal'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  t.fake.script(`
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "lines", count = 300, template = "output row {n}" },
]
`);
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await focusTerminal(t);

  await t.step('Ask for the long answer');
  await t.type('go');
  await t.press('Enter');
  await t.fake.waitEvent('prompt');

  await t.step('The last row is on screen, the first one scrolled away');
  await t.expect.text('output row 300', { timeout: 20000 });
  await t.expect.eventually(async () => (await t.panes())[0]?.status === 'idle', { message: 'the agent is idle again', timeout: 20000 });
  t.expect.ok(!/output row 1\s/.test(await t.page.text()), 'the first row has scrolled out of view');

  await t.step('The cockpit still answers: open the History tab and come back');
  await t.click('exact=History');
  await t.expect.text('No checkpoints yet');
  await t.click('exact=Terminal');
  await t.expect.text('output row 300');
}
