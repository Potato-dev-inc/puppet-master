// Collision Alert: a copy made by Branch from here starts with the save point's files. They are not "new in both".
import { gitProject, turnsScript, savedTurn, selectByTitle, openTab, copyPaneOf, neverShows } from './_area.mjs';

export const meta = {
  id: 'collision.branch-copy-no-false-alert',
  title: 'A Branch copy that only holds the files it was seeded with does not raise a "both made a new file" clash',
  area: 'collision',
  tags: ['collision', 'branch'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  const cwd = gitProject(t, 'app', { 'README.md': 'app\n' });
  t.fake.script(turnsScript([{ prompt: 'add notes', writes: [['notes.txt', 'first notes']] }]));

  await t.step('An agent finishes a turn that adds notes.txt (an untracked new file)');
  await savedTurn(t, cwd, 'add notes');

  await t.step('Branch from the After save point and start the copy');
  await selectByTitle(t, 'add notes');
  await openTab(t, 'History');
  await t.waitFor('exact=Branch from here');
  await t.click('exact=Branch from here', { nth: 0 });
  await t.waitFor('Save point:');
  await t.fill('css=textarea', 'try something else');
  await t.click('exact=Start Nimbus');
  await copyPaneOf(t);

  await t.step('The Inbox must not claim both agents made notes.txt');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await neverShows(t, 'Both made a new file with this name.', 30000);
}
