// Close on a pane that is working: the app asks, and when the person confirms the work already on disk stays and the rest never happens.
import { calm, S, oneTurn, gitProject, openPane, waitStatus, fs, path, PANE_ROW } from './_area.mjs';

export const meta = {
  id: 'work.menu-close-working-pane-keeps-files',
  title: 'Closing a working pane after Close agent keeps the file it already wrote and stops the rest of its job',
  area: 'work',
  tags: ['work', 'menu', 'close'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'closework', { 'README.md': 'demo\n' });
  t.fake.script(oneTurn([S.write('early.txt', 'written early\n'), S.think(30000), S.write('late.txt', 'written late\n'), S.say('job finished')]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await openPane(t, 'Nimbus');
  await pane.write('long job');
  await t.fake.waitEvent((e) => e.ev === 'write_file' && e.path === 'early.txt');
  await waitStatus(t, pane, 'running');

  await t.step('Pane menu > Close asks first');
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('exact=Close');
  await t.expect.text('Close Nimbus?');
  await t.expect.text('It is still running.');

  await t.step('Close agent: the pane is gone');
  await t.click('exact=Close agent');
  await t.expect.count(PANE_ROW, 0);
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'no pane left' });

  await t.step('The early file is still there; the late one never appears');
  t.expect.equal(fs.readFileSync(path.join(cwd, 'early.txt'), 'utf8'), 'written early\n');
  await t.sleep(1500);
  t.expect.ok(!fs.existsSync(path.join(cwd, 'late.txt')), 'late.txt must not exist: the agent was stopped');
}
