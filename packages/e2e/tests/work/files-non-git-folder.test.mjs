// Real work in a folder that is not a git repository: save points still work and the app does not turn the folder into a repo.
import { calm, S, projectDir, writeFiles, workTurn, openHistoryPoint, historyFiles, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-non-git-folder',
  title: 'In a folder without git the turn is still listed in History and no .git folder appears',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = projectDir(t, 'plain');
  writeFiles(cwd, { 'notes.txt': 'first\n' });
  t.expect.ok(!fs.existsSync(path.join(cwd, '.git')), 'the folder starts without git');

  await t.step('The agent changes notes.txt and adds todo.txt');
  await workTurn(t, { cwd, steps: [S.write('notes.txt', 'first\nsecond\n'), S.write('todo.txt', 'buy milk\n'), S.say('noted')] });

  await t.step('History lists both files for the turn');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 2, JSON.stringify(rows));
  t.expect.ok(rows.some((r) => /^M notes\.txt \+1 .0$/.test(r)), JSON.stringify(rows));
  t.expect.ok(rows.some((r) => /^A todo\.txt \+1 .0$/.test(r)), JSON.stringify(rows));

  await t.step('The folder is not a git repository after the turn: the app keeps its own history out of the way');
  t.expect.ok(!fs.existsSync(path.join(cwd, '.git')), 'no .git folder was created in the person\'s folder');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'notes.txt'), 'utf8'), 'first\nsecond\n');
}
