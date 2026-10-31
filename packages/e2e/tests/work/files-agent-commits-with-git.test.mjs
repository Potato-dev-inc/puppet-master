// Real work with git: the agent commits its own change. The commit is real, the app leaves no lock behind and History still lists the file.
import { calm, S, gitProject, git, workTurn, openHistoryPoint, historyFiles, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-agent-commits-with-git',
  title: 'When the agent runs git add and git commit, the commit is real, no lock is left, and History still lists the file',
  area: 'work',
  tags: ['work', 'files', 'git'],
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'commit', { 'README.md': 'demo\n' });
  const before = git(cwd, 'rev-list', '--count', 'HEAD').trim();

  await t.step('The agent writes feature.txt, then runs git add and git commit');
  const { pane } = await workTurn(t, {
    cwd,
    steps: [S.write('feature.txt', 'a feature\n'), S.run('git add -A'), S.run('git -c user.name=Fake -c user.email=fake@example.invalid commit -q -m "add feature"'), S.say('committed')],
  });
  const commit = t.fake.events().filter((e) => e.ev === 'run' && String(e.command).includes('commit')).pop();
  t.expect.equal(commit.code, 0, `git commit exit code (${commit.output})`);

  await t.step('The repository has the new commit and the app left no index.lock');
  t.expect.equal(git(cwd, 'rev-list', '--count', 'HEAD').trim(), String(Number(before) + 1));
  t.expect.equal(git(cwd, 'log', '-1', '--format=%s').trim(), 'add feature');
  t.expect.ok(!fs.existsSync(path.join(cwd, '.git', 'index.lock')), 'no .git/index.lock');
  t.expect.equal(git(cwd, 'status', '--porcelain').split('\n').filter((l) => l.trim() && !l.includes('.puppet-master')).length, 0, 'the working tree is clean apart from the app\'s own folder');

  await t.step('History lists feature.txt for the turn');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.ok(rows.some((r) => /^A feature\.txt \+1 .0$/.test(r)), JSON.stringify(rows));
  t.expect.equal((await pane.info()).status, 'idle');
}
