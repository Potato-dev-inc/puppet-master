// An agent crashes after writing part of its work. The pane says so, the written file stays, and a new agent in the same folder carries on.
import { calm, S, nimbus, oneTurn, gitProject, openPane, waitStatus, groupOfRow, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.long-crash-mid-work-then-restart',
  title: 'After a crash mid-work the file written before it stays and a new agent can finish the job',
  area: 'work',
  tags: ['work', 'long', 'crash'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'crash', { 'README.md': 'demo\n' });
  t.fake.script(oneTurn([S.think(1500), S.write('part1.txt', 'saved before the crash\n'), S.crash('thread main panicked: disk is full', 101)]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openPane(t, 'Nimbus');

  await t.step('The agent writes part one and then crashes');
  await pane.write('big job');
  await waitStatus(t, pane, 'error', 40000);
  await t.expect.text('thread main panicked: disk is full');
  t.expect.equal(await groupOfRow(t, 'big job'), 'Needs you');

  await t.step('The file written before the crash is on disk');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'part1.txt'), 'utf8'), 'saved before the crash\n');

  await t.step('Close the dead pane and start a new agent in the same folder: it finishes the job');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Close');
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'the dead pane is gone' });
  t.fake.script(oneTurn([S.think(500), S.write('part2.txt', 'finished by the second agent\n'), S.say('job finished')]));
  const again = await t.spawnPane('nimbus', { cwd });
  await again.write('finish the job');
  await t.fake.waitEvent((e) => e.ev === 'write_file' && e.path === 'part2.txt');
  await waitStatus(t, again, 'idle');
  t.expect.ok(fs.existsSync(path.join(cwd, 'part1.txt')) && fs.existsSync(path.join(cwd, 'part2.txt')), 'both parts are there');
}
