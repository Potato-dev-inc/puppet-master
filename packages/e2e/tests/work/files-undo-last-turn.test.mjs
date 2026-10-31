// Real work undone: after a turn that edited, added and deleted files, "Undo last turn" puts every file back.
import { calm, S, gitProject, workTurn, openHistoryWith, selectByTitle, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.files-undo-last-turn',
  title: 'Undo last turn restores an edited file, brings back a deleted one and removes a new one',
  area: 'work',
  tags: ['work', 'files', 'undo'],
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'undo', { 'a.txt': 'original\n', 'gone.txt': 'precious\n' });

  await t.step('The agent rewrites a.txt, deletes gone.txt and adds extra.txt');
  await workTurn(t, { cwd, steps: [S.write('a.txt', 'changed by the agent\n'), S.run('del gone.txt'), S.write('extra.txt', 'new\n'), S.say('done')] });
  t.expect.equal(fs.readFileSync(path.join(cwd, 'a.txt'), 'utf8'), 'changed by the agent\n');
  t.expect.ok(!fs.existsSync(path.join(cwd, 'gone.txt')));

  await t.step('History > Undo last turn asks first and says what it does');
  await selectByTitle(t, 'do the work');
  await openHistoryWith(t, 'After turn 1');
  await t.click('exact=Undo last turn');
  await t.expect.text('Undo the last turn?');
  await t.expect.text('Files created since then are removed');
  await t.expect.text('Your current files are saved first');

  await t.step('Confirm: the files are back as they were');
  await t.click('exact=Undo last turn', { nth: 1 });
  await t.expect.eventually(() => fs.readFileSync(path.join(cwd, 'a.txt'), 'utf8') === 'original\n', { message: 'a.txt has its first content again' });
  await t.expect.eventually(() => fs.existsSync(path.join(cwd, 'gone.txt')), { message: 'gone.txt is back' });
  t.expect.equal(fs.readFileSync(path.join(cwd, 'gone.txt'), 'utf8'), 'precious\n');
  t.expect.ok(!fs.existsSync(path.join(cwd, 'extra.txt')), 'extra.txt (created by the turn) is removed');
}
