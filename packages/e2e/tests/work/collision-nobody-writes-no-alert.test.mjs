// Two agents work in one folder at the same time but neither changes a file: nothing to warn about.
import { calm, gitProject } from './_area.mjs';
import { turnsScript, ALERT_TITLE, neverShows } from '../collision/_area.mjs';

export const meta = {
  id: 'work.collision-nobody-writes-no-alert',
  title: 'Two agents working in one folder without changing any file raise no alert',
  area: 'work',
  tags: ['work', 'collision'],
  edge: true,
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  const cwd = gitProject(t, 'quiet', { 'base.txt': 'base\n' });
  t.fake.script(turnsScript([
    { prompt: 'read alpha', think: 50000, say: 'alpha read' },
    { prompt: 'read beta', think: 50000, say: 'beta read' },
  ]));
  const a = await t.spawnPane('nimbus', { cwd });
  const b = await t.spawnPane('nimbus', { cwd });
  await a.write('read alpha');
  await b.write('read beta');
  await t.expect.eventually(async () => (await a.info())?.status === 'running' && (await b.info())?.status === 'running', { message: 'both agents are working' });

  await t.step('Both only read for 35 seconds: the Inbox stays empty');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  await neverShows(t, ALERT_TITLE, 35000);
}
