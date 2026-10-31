// Real work that leaves the working folder: the agent writes ../outside.txt. The app cannot stop another program writing
// there, but it must not list that file as a change of the project or hurt the project's own history.
import { calm, S, gitProject, workTurn, openHistoryPoint, historyFiles, projectDir, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-write-outside-folder',
  title: 'A file the agent writes outside its working folder is not listed as a change of the project',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'inner', { 'README.md': 'demo\n' });
  const outside = path.join(path.dirname(cwd), 'work-files-write-outside-folder-OUTSIDE.txt');
  try { fs.unlinkSync(outside); } catch { /* none */ }

  await t.step('The agent writes one file inside the folder and one next to it, outside');
  await workTurn(t, { cwd, steps: [S.write('inside.txt', 'in\n'), S.write('../work-files-write-outside-folder-OUTSIDE.txt', 'out\n'), S.say('both written')] });
  t.expect.ok(fs.existsSync(outside), 'the outside file exists: nothing refuses an agent that writes there');

  await t.step('History lists only the file inside the project');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 1, JSON.stringify(rows));
  t.expect.ok(/^A inside\.txt \+1 .0$/.test(rows[0]), JSON.stringify(rows));
  t.expect.ok(!fs.existsSync(path.join(cwd, 'work-files-write-outside-folder-OUTSIDE.txt')), 'nothing was copied into the project');
}
