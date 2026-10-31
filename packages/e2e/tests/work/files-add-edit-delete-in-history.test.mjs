// Real work: the agent adds, edits and deletes files in a git folder. History lists exactly those changes for the turn.
import { calm, S, gitProject, workTurn, openHistoryPoint, historyFiles, waitPoint, diffOf } from './_area.mjs';

export const meta = {
  id: 'work.files-add-edit-delete-in-history',
  title: 'A turn that adds, edits and deletes files is listed in History with one row per file',
  area: 'work',
  tags: ['work', 'files', 'smoke'],
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'site', { 'a.txt': 'one\ntwo\n', 'gone.txt': 'bye\n', 'keep.txt': 'stay\n' });

  await t.step('The agent edits a.txt, adds new/dir/n.txt and deletes gone.txt');
  const { pane, list } = await workTurn(t, {
    cwd,
    steps: [S.write('a.txt', 'one\nTWO\nthree\n'), S.write('new/dir/n.txt', 'new file\n'), S.run('del gone.txt'), S.say('all done')],
  });

  await t.step('The app has a save point from after the turn that names exactly those three files');
  const after = list.items.find((c) => c.kind === 'after');
  const changes = await diffOf(t, pane, after);
  t.expect.equal(changes.map((c) => `${c.status} ${c.path}`).sort(), ['A new/dir/n.txt', 'D gone.txt', 'M a.txt']);

  await t.step('Open History and the "After turn 1" row: the rows show the same files with their line counts');
  await openHistoryPoint(t, 'do the work', 'After turn 1');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 3, `three rows, got ${JSON.stringify(rows)}`);
  t.expect.ok(rows.some((r) => /^M a\.txt \+2 .1$/.test(r)), `a.txt row: ${JSON.stringify(rows)}`);
  t.expect.ok(rows.some((r) => /^A new\/dir\/n\.txt \+1 .0$/.test(r)), `n.txt row: ${JSON.stringify(rows)}`);
  t.expect.ok(rows.some((r) => /^D gone\.txt \+0 .1$/.test(r)), `gone.txt row: ${JSON.stringify(rows)}`);
  await t.expect.noText('keep.txt');
}
