// Collision Alert reads git. In a folder without git two agents changing the same file raise no alert (and break nothing).
import { calm, projectDir, writeFiles } from './_area.mjs';
import { turnsScript, ALERT_TITLE, neverShows } from '../collision/_area.mjs';

export const meta = {
  id: 'work.collision-not-a-git-folder-no-alert',
  title: 'In a folder without git, two agents changing one file raise no collision alert',
  area: 'work',
  tags: ['work', 'collision'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = projectDir(t, 'nogit');
  writeFiles(cwd, { 'shared.txt': 'base\n' });
  t.fake.script(turnsScript([
    { prompt: 'edit alpha', think: 4000, writes: [['shared.txt', 'alpha was here']], hold: 60000, say: 'alpha done' },
    { prompt: 'edit beta', think: 4000, writes: [['shared.txt', 'beta was here']], hold: 60000, say: 'beta done' },
  ]));
  const a = await t.spawnPane('nimbus', { cwd });
  const b = await t.spawnPane('nimbus', { cwd });
  await a.write('edit alpha');
  await b.write('edit beta');
  await t.expect.eventually(async () => (await a.info())?.status === 'running' && (await b.info())?.status === 'running', { message: 'both agents are working' });

  await t.step('Both change shared.txt; there is no git to compare with, so the Inbox stays empty');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  await neverShows(t, ALERT_TITLE, 35000);
  await t.expect.noText('overwrite each other');
}
