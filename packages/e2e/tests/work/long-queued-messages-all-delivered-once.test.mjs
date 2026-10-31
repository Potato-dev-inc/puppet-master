// Messages written while the agent works are queued by the app. Each one is delivered once, in order, and the work it asks for is done.
import { calm, S, nimbus, gitProject, openChat, sendChat, prompts, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.long-queued-messages-all-delivered-once',
  title: 'Four messages queued during a long job are each delivered exactly once, in order, and each does its work',
  area: 'work',
  tags: ['work', 'long', 'outbox'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'queue', { 'README.md': 'demo\n' });
  t.fake.script(nimbus([
    { on: 'long job', steps: [S.think(14000), S.write('log.txt', 'long\n', { append: true }), S.say('long job done')] },
    { steps: [S.think(300), S.write('log.txt', '{last_word}\n', { append: true }), S.say('ok {last_word}')] },
  ]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);
  await sendChat(t, 'long job');
  await t.fake.waitEvent('prompt');

  await t.step('While it works, write four more messages');
  for (const w of ['m1', 'm2', 'm3', 'm4']) await sendChat(t, `please ${w}`);
  await t.expect.count('css=[data-testid=chat-pending]', 4);

  await t.step('They go in one at a time after the job; the last one is answered');
  await t.expect.text('ok m4', { timeout: 90000 });
  await waitStatus(t, pane, 'idle');

  await t.step('Five prompts in the right order, none twice; the log shows the work happened in that order');
  t.expect.equal(prompts(t), ['long job', 'please m1', 'please m2', 'please m3', 'please m4']);
  t.expect.equal(fs.readFileSync(path.join(cwd, 'log.txt'), 'utf8'), 'long\nm1\nm2\nm3\nm4\n');
}
