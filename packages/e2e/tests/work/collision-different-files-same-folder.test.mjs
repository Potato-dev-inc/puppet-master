// Two agents in ONE folder change DIFFERENT files while both work. The folder is shared, so the Inbox still warns, and
// it names both files.
import { calm, gitProject, projectDir } from './_area.mjs';
import { turnsScript, ALERT_TITLE } from '../collision/_area.mjs';

export const meta = {
  id: 'work.collision-different-files-same-folder',
  title: 'Two agents in one folder changing different files still raise the shared-folder alert and name both files',
  area: 'work',
  tags: ['work', 'collision'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'shared', { 'base.txt': 'base\n' });
  t.fake.script(turnsScript([
    { prompt: 'edit alpha', think: 4000, writes: [['alpha.txt', 'alpha was here']], hold: 90000, say: 'alpha done' },
    { prompt: 'edit beta', think: 4000, writes: [['beta.txt', 'beta was here']], hold: 90000, say: 'beta done' },
  ]));

  await t.step('Two agents start in the same folder; one writes alpha.txt, the other beta.txt');
  const a = await t.spawnPane('nimbus', { cwd });
  const b = await t.spawnPane('nimbus', { cwd });
  await a.write('edit alpha');
  await b.write('edit beta');

  await t.step('The Inbox card says they work in the same folder and lists both files');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text(ALERT_TITLE, { timeout: 90000 });
  await t.expect.eventually(async () => {
    const m = (await t.page.text()).match(/Changed while both worked: ([^\n]+)\./);
    return !!m && m[1].includes('alpha.txt') && m[1].includes('beta.txt');
  }, { message: async () => `the card lists alpha.txt and beta.txt; the page says: ${(await t.page.text()).split('\n').filter((l) => /Changed while|same folder/.test(l)).join(' | ')}` });
  await t.expect.text('Nothing was paused. You decide.');
  t.expect.equal((await a.info()).status, 'running');
  t.expect.equal((await b.info()).status, 'running');
}
