// An agent whose approval menu has long wording and sits under status bullets: Chat still offers it and a click runs the command.
import { calm, S, oneTurn, gitProject, openChat, sendChat, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.approval-long-wording-menu',
  title: 'A long-worded numbered approval (bullets above it) is answered from Chat and the command then runs',
  area: 'work',
  tags: ['work', 'approval'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'longmenu', { 'README.md': 'demo\n' });
  const maker = path.join(t.app.dirs.root, 'make-ran.cjs');
  fs.writeFileSync(maker, "require('fs').writeFileSync('ran.txt', 'the approved command ran\\n');");
  t.fake.script(oneTurn(
    [S.think(300), S.approve({ kind: 'bash', command: `node "${maker}"`, allow: [S.run(`node "${maker}"`), S.say('command finished')], deny: [S.say('command cancelled')] })],
    'approvals = "bullets_numbered"',
  ));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);

  await t.step('The agent stops with the long numbered menu');
  await sendChat(t, 'run my script');
  await t.expect.text('Answer the agent');
  await t.expect.visible('exact=Yes, run command');
  await t.expect.visible('exact=No, cancel');
  t.expect.ok(!fs.existsSync(path.join(cwd, 'ran.txt')), 'not run yet');

  await t.step('Click Yes, run command: the script runs and writes its file');
  await t.click('exact=Yes, run command');
  await t.expect.text('command finished');
  await waitStatus(t, pane, 'idle');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'ran.txt'), 'utf8'), 'the approved command ran\n');
}
