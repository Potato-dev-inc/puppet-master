// The session rail beside a pane: Where (agent, folder) and Context. The ] key and the header button hide and show it.
import { calm, openPane } from './_area.mjs';

export const meta = {
  id: 'panes.rail-where-and-toggle',
  title: 'The side rail shows Where and Context, and ] hides and shows it',
  area: 'panes',
  tags: ['rail', 'keyboard'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus', { cwd: t.project('rail-folder') });
  await openPane(t, 'Nimbus');

  await t.step('The rail is open: Where and Context');
  await t.waitFor('css=.ck-rail');
  await t.expect.text('Where');
  await t.expect.visible('exact=Agent', { within: 'css=.ck-rail' });
  await t.expect.visible('exact=Folder', { within: 'css=.ck-rail' });
  await t.expect.text('rail-folder');
  await t.expect.text('Context');
  await t.expect.text('Not reported by this agent');

  await t.step('Press ]: the rail hides');
  await t.press(']');
  await t.expect.notVisible('css=.ck-rail');
  await t.expect.visible('aria=Show session details');

  await t.step('The header button shows it again');
  await t.click('aria=Show session details');
  await t.waitFor('css=.ck-rail');
  await t.expect.visible('aria=Hide session details');

  await t.step('The choice is remembered after a reload');
  await t.press(']');
  await t.expect.notVisible('css=.ck-rail');
  await t.page.reload();
  await t.expect.text('Puppet Master');
  await openPane(t, 'Nimbus');
  await t.expect.notVisible('css=.ck-rail');
  await t.press(']');
  await t.waitFor('css=.ck-rail');
}
