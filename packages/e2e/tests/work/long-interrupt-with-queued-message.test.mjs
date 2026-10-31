// A message is waiting in Chat while the agent works; the person interrupts the job. The waiting message must not be lost.
import { calm, S, nimbus, gitProject, openChat, sendChat, prompts, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.long-interrupt-with-queued-message',
  title: 'A message waiting in Chat is delivered once after the person interrupts the job',
  area: 'work',
  tags: ['work', 'long', 'interrupt', 'outbox'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'intq', { 'README.md': 'demo\n' });
  t.fake.script(nimbus([
    { on: 'long job', steps: [S.write('started.txt', 'yes\n'), S.think(60000), S.say('long job done')] },
    { steps: [S.think(300), S.write('after.txt', '{last_word}\n'), S.say('handled {last_word}')] },
  ]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);
  await sendChat(t, 'long job');
  await t.fake.waitEvent('prompt');
  await waitStatus(t, pane, 'running');

  await t.step('A message waits behind the job');
  await sendChat(t, 'then do this');
  await t.expect.text('The agent is working. Sends when the turn ends.');

  await t.step('Interrupt from the pane menu');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('Interrupt');
  await t.fake.waitEvent('interrupted');

  await t.step('The waiting message is delivered once and the agent does what it asks');
  await t.expect.text('handled this', { timeout: 45000 });
  t.expect.equal(prompts(t), ['long job', 'then do this']);
  t.expect.equal(fs.readFileSync(path.join(cwd, 'after.txt'), 'utf8'), 'this\n');
  await waitStatus(t, pane, 'idle');
  await t.expect.noText('Sends when the turn ends');
}
