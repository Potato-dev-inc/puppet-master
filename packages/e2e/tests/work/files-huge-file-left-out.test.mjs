// Real work: the agent creates a 25 MB file. Save points leave files over 20 MB out; the small files of the turn still count and the app stays usable.
import { calm, S, gitProject, workTurn, openHistoryPoint, historyFiles, checkpoints, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-huge-file-left-out',
  title: 'A 25 MB file is left out of the save point while the small files of the same turn are kept',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'big', { 'README.md': 'demo\n' });
  const maker = path.join(t.app.dirs.root, 'make-big.cjs');
  fs.writeFileSync(maker, "require('fs').writeFileSync('huge.dat', Buffer.alloc(25 * 1024 * 1024, 65));");

  await t.step('The agent writes a 25 MB file and a small notes file');
  await workTurn(t, { cwd, steps: [S.run(`node "${maker}"`), S.write('notes.txt', 'small\n'), S.say('done')] });
  t.expect.equal(fs.statSync(path.join(cwd, 'huge.dat')).size, 25 * 1024 * 1024);

  await t.step('The save points are fine (no "too big" state) and History lists only the small file');
  const l = await checkpoints(t, { id: (await t.panes())[0].id });
  t.expect.equal(l.state, 'ok');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 1, JSON.stringify(rows));
  t.expect.ok(/^A notes\.txt \+1 .0$/.test(rows[0]), JSON.stringify(rows));
  await t.expect.noText('huge.dat');
  await t.expect.noText('Undo is off for this folder');
}
