// The agent waits for an approval and the person closes its pane. The app asks first; closing ends the agent and nothing is written.
import { calm, S, oneTurn, gitProject, openChat, sendChat, waitStatus, fs, path, PANE_ROW } from './_area.mjs';

export const meta = {
  id: 'work.approval-close-pane-while-asking',
  title: 'Closing a pane that waits for approval asks first, then ends the agent without writing anything',
  area: 'work',
  tags: ['work', 'approval', 'close'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'closeask', { 'README.md': 'demo\n' });
  t.fake.script(oneTurn(
    [S.think(300), S.approve({ kind: 'bash', command: 'echo risky', allow: [S.write('risky.txt', 'x\n'), S.say('did the risky thing')], deny: [S.say('refused')] })],
    'approvals = "menus"',
  ));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);
  await sendChat(t, 'do the risky thing');
  await t.expect.text('Answer the agent');
  await waitStatus(t, pane, 'waiting_input');

  await t.step('Pane menu > Close: the app asks before ending an agent that is waiting');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Close');
  await t.expect.text('Close Nimbus?');
  await t.click('exact=Keep running');
  await t.expect.count(PANE_ROW, 1);
  t.expect.equal((await pane.info()).status, 'waiting_input', 'still waiting');

  await t.step('Close it for real');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Close');
  await t.click('exact=Close agent');
  await t.expect.count(PANE_ROW, 0);
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'no pane left' });
  t.expect.ok(!fs.existsSync(path.join(cwd, 'risky.txt')), 'nothing was written');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'approval_answer').length, 0, 'the agent was never answered');
}
