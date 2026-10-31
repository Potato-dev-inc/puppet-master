// Work stopped half way: part one is written, the person interrupts from the pane menu before part two, then asks to continue.
import { calm, S, nimbus, gitProject, openPane, waitStatus, toasts, fs, path } from './_area.mjs';

export const meta = {
  id: 'work.long-interrupt-then-continue',
  title: 'Interrupting a job half way keeps what was written, never writes the rest, and a follow-up finishes it',
  area: 'work',
  tags: ['work', 'long', 'interrupt'],
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'half', { 'README.md': 'demo\n' });
  t.fake.script(nimbus([
    { on: 'build it', steps: [S.write('part1.txt', 'first half\n'), S.think(60000), S.write('part2.txt', 'second half\n'), S.say('built it all')] },
    { on: 'carry on', steps: [S.think(300), S.write('part2.txt', 'second half\n'), S.say('carried on')] },
  ]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openPane(t, 'Nimbus');

  await t.step('The agent writes part one and keeps working');
  await pane.write('build it');
  await t.fake.waitEvent((e) => e.ev === 'write_file' && e.path === 'part1.txt');
  await waitStatus(t, pane, 'running');

  await t.step('Pane menu > Interrupt');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('Interrupt');
  await t.fake.waitEvent('interrupted');
  await waitStatus(t, pane, 'idle');

  await t.step('Part one is on disk, part two never was');
  t.expect.ok(fs.existsSync(path.join(cwd, 'part1.txt')));
  t.expect.ok(!fs.existsSync(path.join(cwd, 'part2.txt')));

  await t.step('Ask it to carry on: it finishes the job');
  await pane.write('carry on');
  await t.expect.text('carried on', { timeout: 30000 });
  await waitStatus(t, pane, 'idle');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'part2.txt'), 'utf8'), 'second half\n');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'part1.txt'), 'utf8'), 'first half\n');
}
