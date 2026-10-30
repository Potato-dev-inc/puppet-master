// "Choose myself" goes back to the ordinary agent list; nothing was started and the menu works as before.
import { openNewMenu, sclick, stable, waitClean } from '../night/_area.mjs';

export const meta = {
  id: 'keepgoing.pick-for-me-choose-myself',
  title: 'Choose myself goes back to the normal list and starting an agent still works',
  area: 'keepgoing',
  tags: ['pick-for-me'],
  edge: true,
};

export async function run(t) {
  await waitClean(t); // the helper pane the launcher used to open the window
  const before = (await t.panes()).length;
  await openNewMenu(t);
  await sclick(t, 'Pick for me');
  await t.waitFor('css=.ck-pick');

  await t.step('Press Choose myself');
  await t.click('exact=Choose myself');
  await t.expect.notVisible('css=.ck-pick');

  await t.step('The ordinary list is back');
  await stable(t, 'exact=Nimbus');
  for (const name of ['Claude Code', 'Codex CLI', 'PowerShell', 'Nimbus']) await t.expect.visible(`exact=${name}`);
  t.expect.equal((await t.panes()).length, before, 'nothing was started');

  await t.step('Start the fake agent from the list');
  await t.click('exact=Nimbus');
  await t.expect.eventually(async () => (await t.panes()).length === before + 1, { message: 'one pane opens' });
  t.expect.ok((await t.panes()).some((p) => p.agent_type === 'nimbus'), 'it is a Nimbus pane');
}
