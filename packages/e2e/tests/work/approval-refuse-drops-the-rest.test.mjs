// Mid-work approval refused: the rest of the turn (a write that comes after the question) is dropped, and the next message works normally.
import { calm, S, nimbus, gitProject, openChat, sendChat, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.approval-refuse-drops-the-rest',
  title: 'Deny in Chat drops the rest of the turn: the later write never happens and a new message works',
  area: 'work',
  tags: ['work', 'approval'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'refuse', { 'README.md': 'demo\n' });
  t.fake.script(nimbus([
    {
      on: 'risky',
      steps: [
        S.think(300),
        S.approve({ kind: 'bash', command: 'rm -rf build', allow: [S.say('removed build')], deny: [S.say('left build alone')] }),
        S.write('after-question.txt', 'should not exist after a deny\n'),
        S.say('turn fully finished'),
      ],
    },
    { on: 'safe', steps: [S.think(300), S.write('safe.txt', 'ok\n'), S.say('safe work done')] },
  ], 'approvals = "menus"'));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);

  await t.step('The agent asks before a risky command: Deny');
  await sendChat(t, 'do the risky thing');
  await t.expect.text('Answer the agent');
  await t.click('exact=Deny');
  await t.expect.text('left build alone');
  await waitStatus(t, pane, 'idle');

  await t.step('The write that came after the question never happened and the turn did not claim to finish');
  t.expect.ok(!fs.existsSync(path.join(cwd, 'after-question.txt')), 'after-question.txt must not exist');
  await t.expect.noText('turn fully finished');

  await t.step('A new message works as usual');
  await sendChat(t, 'do the safe thing');
  await t.expect.text('safe work done');
  t.expect.ok(fs.existsSync(path.join(cwd, 'safe.txt')));
}
