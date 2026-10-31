// Six agents work at the same time in six folders. All of them finish, each file has its own content, nothing needs the person.
import { calm, S, oneTurn, gitProject, waitStatus, groupOfRow, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.many-six-panes-work-at-once',
  title: 'Six agents working at once each write their own file, and the sidebar settles with all six idle',
  area: 'work',
  tags: ['work', 'many'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(4000), S.write('out.txt', 'result of {last_word}\n'), S.say('finished {last_word}')]));
  const words = ['alpha', 'bravo', 'charlie', 'delta', 'echo', 'foxtrot'];
  const panes = [];
  for (const w of words) panes.push({ w, cwd: gitProject(t, w, { 'README.md': `${w}\n` }), pane: null });
  for (const p of panes) p.pane = await t.spawnPane('nimbus', { cwd: p.cwd });

  await t.step('Give all six a job one after the other, without waiting');
  for (const p of panes) await p.pane.write(`job ${p.w}`);
  await t.expect.eventually(async () => (await t.panes()).filter((x) => x.status === 'running').length === 6, { timeout: 20000, message: 'six panes working at once' });

  await t.step('The sidebar lists them under Working');
  await t.expect.eventually(async () => (await groupOfRow(t, 'job alpha')) === 'Working' && (await groupOfRow(t, 'job foxtrot')) === 'Working', { message: 'all six rows are under Working' });

  await t.step('All six finish; each folder has its own file');
  for (const p of panes) await waitStatus(t, p.pane, 'idle', 60000);
  for (const p of panes) t.expect.equal(fs.readFileSync(path.join(p.cwd, 'out.txt'), 'utf8'), `result of ${p.w}\n`, `file of ${p.w}`);

  await t.step('Nothing needs the person');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  t.expect.equal((await t.panes()).filter((x) => x.status === 'idle').length, 6);
}
