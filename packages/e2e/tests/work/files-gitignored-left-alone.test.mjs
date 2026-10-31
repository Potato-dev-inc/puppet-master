// Real work: the agent creates files that .gitignore names (a build folder, a log). Save points leave them out; the real source file is kept.
import { calm, S, gitProject, workTurn, openHistoryPoint, historyFiles, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-gitignored-left-alone',
  title: 'Files that .gitignore names (a build folder, a log) are not listed as changes',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'ignore', { '.gitignore': 'build/\n*.log\n', 'src.txt': 'code\n' });

  await t.step('The agent writes src.txt, build/out.js and debug.log');
  await workTurn(t, { cwd, steps: [S.write('src.txt', 'code\nmore code\n'), S.write('build/out.js', 'compiled\n'), S.write('debug.log', 'noise\n'), S.say('built')] });
  t.expect.ok(fs.existsSync(path.join(cwd, 'build', 'out.js')) && fs.existsSync(path.join(cwd, 'debug.log')), 'the ignored files are on disk');

  await t.step('History lists src.txt only');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 1, JSON.stringify(rows));
  t.expect.ok(/^M src\.txt \+1 .0$/.test(rows[0]), JSON.stringify(rows));
}
