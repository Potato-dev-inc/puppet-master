// Long multi-turn work: five messages in a row from Chat, each one a turn that appends to a log. Every turn is answered, in order.
import { calm, S, nimbus, gitProject, openChat, sendChat, prompts, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.long-five-turns-one-pane',
  title: 'Five turns in one pane each do their work in order and Chat shows every answer',
  area: 'work',
  tags: ['work', 'long'],
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'five', { 'README.md': 'demo\n' });
  t.fake.script(nimbus([{ steps: [S.think(600), S.write('log.txt', '{last_word}\n', { append: true }), S.say('finished {last_word}')] }]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openChat(t);

  const words = ['one', 'two', 'three', 'four', 'five'];
  for (const w of words) {
    await t.step(`Turn: step ${w}`);
    await sendChat(t, `step ${w}`);
    await t.expect.text(`finished ${w}`, { timeout: 30000 });
    await waitStatus(t, pane, 'idle');
  }

  await t.step('The agent got exactly five messages in order and the log has five lines in the same order');
  t.expect.equal(prompts(t), words.map((w) => `step ${w}`));
  t.expect.equal(fs.readFileSync(path.join(cwd, 'log.txt'), 'utf8'), words.join('\n') + '\n');

  await t.step('Chat still shows all five answers');
  for (const w of words) await t.expect.text(`finished ${w}`);
}
