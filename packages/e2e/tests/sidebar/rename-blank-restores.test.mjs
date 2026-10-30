// Edge case: saving an empty name clears the pane's own name; the agent's name is back.
import { calm, PANE_TITLE, paneHeading, sidebarTitles } from './_area.mjs';

export const meta = {
  id: 'sidebar.rename-blank-restores',
  title: 'Saving an empty name gives the pane its agent name back',
  area: 'sidebar',
  tags: ['rename'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await t.click('exact=Nimbus', { within: 'css=.ck-side' });
  await t.waitFor(PANE_TITLE);

  await t.step('Give it a name');
  await t.page.doubleClick(PANE_TITLE);
  await t.page.fill('aria=Pane name', 'Temporary name');
  await t.press('Enter');
  await t.expect.eventually(async () => (await paneHeading(t)) === 'Temporary name', { message: 'named' });

  await t.step('Rename it to nothing (spaces only)');
  await t.page.doubleClick(PANE_TITLE);
  await t.page.fill('aria=Pane name', '   ');
  await t.press('Enter');

  await t.step('The agent name is back in the header and the sidebar');
  await t.expect.eventually(async () => (await paneHeading(t)) === 'Nimbus', { message: 'the header says Nimbus again' });
  await t.expect.eventually(async () => (await sidebarTitles(t)).join() === 'Nimbus', { message: 'and the sidebar too' });
}
