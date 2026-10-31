// Real work in progress: while the agent works on a second turn, History refuses to restore ("Interrupt the agent first").
import { calm, S, nimbus, gitProject, waitStatus, waitPoint, selectByTitle, openHistoryWith, fs, path } from './_area.mjs';
import { isDisabled } from '../whatbroke/_area.mjs';

export const meta = {
  id: 'work.files-restore-blocked-while-working',
  title: 'While the agent works, Restore and Undo last turn are off and say to interrupt first',
  area: 'work',
  tags: ['work', 'files', 'undo'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'busy', { 'a.txt': 'start\n' });
  t.fake.script(nimbus([
    { on: 'quick job', steps: [S.think(2500), S.write('a.txt', 'start\nquick\n'), S.say('quick done')] },
    { on: 'long job', steps: [S.think(40000), S.write('a.txt', 'start\nquick\nlong\n'), S.say('long done')] },
  ]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await pane.write('quick job');
  await waitStatus(t, pane, 'running');
  await waitStatus(t, pane, 'idle', 60000);
  await waitPoint(t, pane, 'after');

  await t.step('A long second job starts');
  await pane.write('long job');
  await waitStatus(t, pane, 'running');
  await selectByTitle(t, 'quick job');
  await openHistoryWith(t, 'After turn 1');

  await t.step('The history note says to interrupt first and the buttons are off');
  await t.expect.text('The agent is working: interrupt it before restoring.');
  t.expect.ok(await isDisabled(t, 'exact=Restore', { nth: 0 }), 'Restore is off');
  t.expect.ok(await isDisabled(t, 'exact=Undo last turn'), 'Undo last turn is off');

  await t.step('Nothing was touched on disk');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'a.txt'), 'utf8'), 'start\nquick\n');
}
