// Real work over two turns in one pane: each turn's save point lists only that turn's own changes.
import { calm, S, nimbus, gitProject, waitStatus, waitPoint, openHistoryPoint, historyFiles } from './_area.mjs';

export const meta = {
  id: 'work.files-two-turns-own-changes',
  title: 'The second turn\'s save point lists only the second turn\'s changes, not the first\'s',
  area: 'work',
  tags: ['work', 'files'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'two', { 'a.txt': 'start\n' });
  t.fake.script(nimbus([
    { on: 'first job', steps: [S.think(2500), S.write('a.txt', 'start\nfirst\n'), S.say('first done')] },
    { on: 'second job', steps: [S.think(2500), S.write('b.txt', 'second\n'), S.say('second done')] },
  ]));
  const pane = await t.spawnPane('nimbus', { cwd });

  await t.step('Turn 1: the agent edits a.txt');
  await pane.write('first job');
  await waitStatus(t, pane, 'running');
  await waitStatus(t, pane, 'idle', 60000);
  await waitPoint(t, pane, 'after');

  await t.step('Turn 2: the agent adds b.txt');
  await pane.write('second job');
  await t.expect.eventually(async () => ((await t.bridge.get(`/panes/${pane.id}/checkpoints`)).body.items || []).some((c) => c.label === 'After turn 2'), { timeout: 90000, message: 'an "After turn 2" save point' });

  await t.step('History: the After turn 2 row lists b.txt only');
  await openHistoryPoint(t, 'first job', 'After turn 2');
  const rows = await historyFiles(t);
  t.expect.equal(rows.length, 1, JSON.stringify(rows));
  t.expect.ok(/^A b\.txt \+1 .0$/.test(rows[0]), JSON.stringify(rows));
}
