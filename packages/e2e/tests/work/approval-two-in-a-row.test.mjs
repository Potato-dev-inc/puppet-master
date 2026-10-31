// One turn with two approvals in a row: the person allows the first and denies the second; the work matches the answers.
import { calm, S, oneTurn, gitProject, openChat, sendChat, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.approval-two-in-a-row',
  title: 'Two approvals in one turn: allow the first, deny the second, and only the first one\'s work happens',
  area: 'work',
  tags: ['work', 'approval'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'two', { 'README.md': 'demo\n' });
  t.fake.script(oneTurn([
    S.think(300),
    S.approve({ kind: 'bash', command: 'echo first', allow: [S.write('first.txt', 'first\n'), S.say('first approved')], deny: [S.say('first refused')] }),
    S.approve({ kind: 'bash', command: 'echo second', allow: [S.write('second.txt', 'second\n'), S.say('second approved')], deny: [S.say('second refused')] }),
    S.say('both questions answered'),
  ], 'approvals = "menus"'));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);
  await sendChat(t, 'do both things');

  await t.step('First question: Allow once');
  await t.expect.text('Answer the agent');
  await t.click('exact=Allow once');
  await t.expect.text('first approved');

  await t.step('Second question: Deny');
  await t.fake.waitEvent((e) => e.ev === 'approval_shown' && e.command === 'echo second');
  await t.expect.visible('exact=Deny');
  await t.click('exact=Deny');
  await t.expect.text('second refused');
  await waitStatus(t, pane, 'idle');

  await t.step('Only the first one\'s file exists');
  t.expect.ok(fs.existsSync(path.join(cwd, 'first.txt')));
  t.expect.ok(!fs.existsSync(path.join(cwd, 'second.txt')));
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'approval_answer').length, 2, 'two answers reached the agent');
}
