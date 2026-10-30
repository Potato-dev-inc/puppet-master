// Edge case: the open tab belongs to the pane; picking another pane starts on its Terminal tab.
import { calm, openPane, selectedTab } from './_area.mjs';

export const meta = {
  id: 'panes.tab-resets-on-other-pane',
  title: 'Another pane opens on its Terminal tab, not on the tab the last pane was on',
  area: 'panes',
  tags: ['tabs'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });

  await t.step('Open the first pane on its History tab');
  await openPane(t, 'Nimbus 1');
  await t.click('exact=History');
  await t.expect.text('No checkpoints yet');

  await t.step('Pick the second pane: Terminal');
  await openPane(t, 'Nimbus 2');
  await t.expect.eventually(async () => (await selectedTab(t)) === 'Terminal', { message: 'the Terminal tab is selected' });
  await t.expect.text('NIMBUS 0.4.2');
  await t.expect.noText('No checkpoints yet');
}
