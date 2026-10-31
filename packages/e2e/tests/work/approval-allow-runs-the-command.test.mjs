// Mid-work approval answered from Chat: nothing happens until the person allows it; then the command's effect is on disk.
import { calm, S, oneTurn, gitProject, openChat, sendChat, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.approval-allow-runs-the-command',
  title: 'The agent writes only after the person clicks Allow once in Chat',
  area: 'work',
  tags: ['work', 'approval'],
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'allow', { 'README.md': 'demo\n' });
  t.fake.script(oneTurn(
    [S.think(300), S.approve({ kind: 'bash', command: 'echo deploy', allow: [S.write('deployed.txt', 'deployed\n'), S.say('deployed it')], deny: [S.say('did not deploy')] })],
    'approvals = "menus"',
  ));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);

  await t.step('Ask for the deploy: the agent stops and asks');
  await sendChat(t, 'deploy it');
  await t.expect.text('Answer the agent');
  await waitStatus(t, pane, 'waiting_input');
  t.expect.ok(!fs.existsSync(path.join(cwd, 'deployed.txt')), 'nothing is written while the question is open');

  await t.step('Click Allow once: the agent carries on and writes the file');
  await t.click('exact=Allow once');
  await t.expect.text('deployed it');
  await waitStatus(t, pane, 'idle');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'deployed.txt'), 'utf8'), 'deployed\n');
  await t.expect.notVisible('exact=Allow once');
}
