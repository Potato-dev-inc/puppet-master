// Edge case: the agent program crashes (a panic line, exit code 101). The pane shows Error, asks for attention, the panic text stays
// readable, and after closing it a new agent starts normally.
import { calm, openPane, groupOfRow, sidebarTitles } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'panes.agent-crash',
  title: 'A crashing agent shows Error and Needs you, and a new agent can be started after it',
  area: 'panes',
  tags: ['states', 'crash'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  t.fake.script(`
boot_ms = 100
inline = true
[[turn]]
on_prompt = "explode"
steps = [
  { do = "say", text = "about to fail" },
  { do = "crash", message = "kaboom internal error", code = 101 },
]
`);
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');

  await t.step('Ask the agent to do the thing that crashes it');
  await pane.write('explode');

  await t.step('The pane is in the Error state and needs you');
  await t.expect.eventually(async () => (await pane.info())?.status === 'error', { message: 'the app reports an error', timeout: 15000 });
  await t.expect.text(/Nimbus · Error/);
  await t.expect.text('Inbox');
  t.expect.equal(await groupOfRow(t, 'explode'), 'Needs you', 'the row is under Needs you');

  await t.step('The panic text is still readable in the terminal');
  await t.expect.text('kaboom internal error');

  await t.step('Close the dead pane: no question, it is not running');
  await t.click('aria=Pane actions');
  await t.click('exact=Close');
  await t.expect.count(PANE_ROW, 0);
  await t.expect.text('No agents running.');

  await t.step('Start a fresh agent: it works');
  t.fake.resetScript();
  await t.spawnPane('nimbus');
  await t.expect.eventually(async () => (await sidebarTitles(t)).join() === 'Nimbus', { message: 'a new Nimbus row' });
  await openPane(t, 'Nimbus');
  await t.expect.text('Type a task...');
  await t.expect.noText('kaboom internal error');
}
