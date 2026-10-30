// Collision Alert: two agents in two different folders never collide, even for the same file name.
import { gitProject, twoWriters, ALERT_TITLE, neverShows } from './_area.mjs';

export const meta = {
  id: 'collision.different-folders-no-alert',
  title: 'Two agents changing the same file name in two different folders raise no alert',
  area: 'collision',
  tags: ['collision'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await t.step('Two agents work at the same time, each in its own git folder, writing shared.txt');
  const one = gitProject(t, 'app-one', { 'shared.txt': 'base\n' });
  const two = gitProject(t, 'app-two', { 'shared.txt': 'base\n' });
  await twoWriters(t, { cwdA: one, cwdB: two });

  await t.step('The Inbox stays empty while both work');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  await neverShows(t, ALERT_TITLE, 35000);
  await t.expect.noText('overwrite each other');
}
