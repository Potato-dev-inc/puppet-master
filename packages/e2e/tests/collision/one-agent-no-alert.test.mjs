// Collision Alert: one working agent in a folder is nobody to collide with.
import { gitProject, turnsScript, ALERT_TITLE, neverShows } from './_area.mjs';

export const meta = {
  id: 'collision.one-agent-no-alert',
  title: 'A second agent that only sits idle in the folder raises no alert',
  area: 'collision',
  tags: ['collision'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  const cwd = gitProject(t, 'app', { 'shared.txt': 'base\n' });
  t.fake.script(turnsScript([{ prompt: 'edit alpha', think: 4000, writes: [['shared.txt', 'alpha was here']], hold: 60000 }]));

  await t.step('Two agents are in one folder, but only one of them is working');
  const a = await t.spawnPane('nimbus', { cwd });
  const idle = await t.spawnPane('nimbus', { cwd });
  await a.write('edit alpha');
  await t.expect.eventually(async () => (await a.info())?.status === 'running', { message: 'the first agent is working' });
  t.expect.equal((await idle.info()).status, 'idle');

  await t.step('The Inbox stays empty: a folder is looked at only while two agents work');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  await neverShows(t, 'working in the same folder', 35000);
  await t.expect.noText(ALERT_TITLE);
}
