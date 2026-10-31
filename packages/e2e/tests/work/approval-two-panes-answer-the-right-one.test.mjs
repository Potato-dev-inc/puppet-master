// Two agents wait for approval at the same time. Answering one's card in Chat does not touch the other.
import { calm, S, oneTurn, gitProject, openChat, sendChat, waitStatus, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.approval-two-panes-answer-the-right-one',
  title: 'With two agents waiting for approval, Allow once in one pane lets only that agent write',
  area: 'work',
  tags: ['work', 'approval', 'two-panes'],
  edge: true,
  timeout: 120000,
};

const agentScript = (file) => oneTurn(
  [S.think(300), S.approve({ kind: 'bash', command: `echo ${file}`, allow: [S.write(file, 'allowed\n'), S.say(`wrote ${file}`)], deny: [S.say('refused')] })],
  'approvals = "menus"',
);

export async function run(t) {
  await calm(t);
  const cwdA = gitProject(t, 'alpha', { 'README.md': 'a\n' });
  const cwdB = gitProject(t, 'beta', { 'README.md': 'b\n' });
  t.fake.script(agentScript('alpha.txt'));
  const a = await t.spawnPane('nimbus', { cwd: cwdA });
  t.fake.script(agentScript('beta.txt'));
  const b = await t.spawnPane('nimbus', { cwd: cwdB });

  await t.step('Both agents are asked for something and both stop at an approval');
  await a.write('job alpha');
  await b.write('job beta');
  await waitStatus(t, a, 'waiting_input');
  await waitStatus(t, b, 'waiting_input');

  await t.step('Open the beta agent in Chat and allow it');
  await openChat(t, 'job beta');
  await t.expect.visible('exact=Allow once');
  await t.click('exact=Allow once');
  await t.expect.text('wrote beta.txt');

  await t.step('Only beta wrote its file; alpha still waits');
  t.expect.ok(fs.existsSync(path.join(cwdB, 'beta.txt')), 'beta.txt exists');
  t.expect.ok(!fs.existsSync(path.join(cwdA, 'alpha.txt')), 'alpha.txt does not exist yet');
  t.expect.equal((await a.info()).status, 'waiting_input');

  await t.step('Now allow alpha too');
  await openChat(t, 'job alpha');
  await t.click('exact=Allow once');
  await t.expect.text('wrote alpha.txt');
  t.expect.ok(fs.existsSync(path.join(cwdA, 'alpha.txt')));
}
