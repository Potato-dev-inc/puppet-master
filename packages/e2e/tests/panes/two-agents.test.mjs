// Two panes side by side in the sidebar; each one keeps its own folder.
import { PANE_ROW, selectPane } from '../_helpers.mjs';

import { calm } from './_area.mjs';

export const meta = {
  id: 'panes.two-agents',
  title: 'Two agent panes are listed and each shows its own folder',
  area: 'panes',
  tags: [],
};

const folderShown = async (t) => (await t.page.text()).match(/Folder\s*\n([^\n]+)/)?.[1] || '';

export async function run(t) {
  await calm(t);
  await t.step('Start two fake agents in two folders');
  await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });
  await t.expect.count(PANE_ROW, 2);

  await t.step('Open the first pane');
  await selectPane(t, 'Nimbus 1');
  await t.expect.eventually(async () => /alpha|beta/.test(await folderShown(t)), { message: 'a folder is shown' });
  const first = (await folderShown(t)).includes('alpha') ? 'alpha' : 'beta';
  const other = first === 'alpha' ? 'beta' : 'alpha';

  await t.step('Open the second pane: the folder changes');
  await selectPane(t, 'Nimbus 2');
  await t.expect.eventually(async () => (await folderShown(t)).includes(other), { message: `the second pane should show the folder "${other}"` });
}
