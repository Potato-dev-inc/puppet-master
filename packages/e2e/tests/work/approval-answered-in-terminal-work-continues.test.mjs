// A numbered approval is answered at the terminal (the digit typed into the pane) instead of the Chat card: the card goes away
// by itself and the work that follows the approval happens.
import { calm, S, oneTurn, gitProject, openChat, sendChat, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.approval-answered-in-terminal-work-continues',
  title: 'Typing the answer in the terminal clears the Chat card and the agent goes on to write its file',
  area: 'work',
  tags: ['work', 'approval'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'terminal-answer', { 'README.md': 'demo\n' });
  t.fake.script(oneTurn(
    [S.think(300), S.approve({ kind: 'bash', command: 'echo migrate', allow: [S.write('migrated.txt', 'migrated\n'), S.say('migration done')], deny: [S.say('migration skipped')] })],
    'approvals = "menus"',
  ));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);
  await sendChat(t, 'migrate the data');
  await t.expect.text('Answer the agent');
  await waitStatus(t, pane, 'waiting_input');

  await t.step('The person types 1 straight into the pane (the same as pressing it at the terminal)');
  await pane.write('1', { enter: false });
  await t.fake.waitEvent('approval_answer');

  await t.step('The card goes away by itself and the agent writes its file');
  await t.expect.eventually(async () => (await pane.info())?.status === 'idle', { timeout: 30000, message: 'the agent finished' });
  await t.expect.notVisible('exact=Allow once');
  await t.expect.noText('Answer the agent');
  await t.expect.text('migration done');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'migrated.txt'), 'utf8'), 'migrated\n');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'approval_answer').length, 1, 'answered once');
}
