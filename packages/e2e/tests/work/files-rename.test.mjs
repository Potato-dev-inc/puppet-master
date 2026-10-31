// Real work: the agent renames a file. History shows the old name as removed and the new name as added.
import { calm, S, gitProject, workTurn, openHistoryPoint, historyFiles, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-rename',
  title: 'A renamed file shows as the old name removed and the new name added',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'rename', { 'old-name.txt': 'same content here\nsecond line\n', 'other.txt': 'x\n' });

  await t.step('The agent renames old-name.txt to new-name.txt');
  await workTurn(t, { cwd, steps: [S.run('ren old-name.txt new-name.txt'), S.say('renamed')] });
  t.expect.ok(fs.existsSync(path.join(cwd, 'new-name.txt')) && !fs.existsSync(path.join(cwd, 'old-name.txt')));

  await t.step('History shows exactly two rows: one removed, one added');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 2, JSON.stringify(rows));
  t.expect.ok(rows.some((r) => /^D old-name\.txt/.test(r)), JSON.stringify(rows));
  t.expect.ok(rows.some((r) => /^A new-name\.txt/.test(r)), JSON.stringify(rows));
}
