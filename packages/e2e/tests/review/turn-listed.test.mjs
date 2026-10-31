// Review desk: an agent turn that changed files in your own folder shows up as a row, with a plain reason.
import { gitProject, writingTurn, openDeskWithPane, rowText, deskHead } from './_area.mjs';

export const meta = {
  id: 'review.turn-listed',
  title: 'A finished agent turn that changed files is listed on the Review desk as low risk with no flags',
  area: 'review',
  tags: ['review', 'smoke'],
  timeout: 90000,
};

export async function run(t) {
  await t.step('An agent finishes a turn that adds one new file to a git folder');
  const cwd = gitProject(t, 'site', { 'README.md': 'site\n' });
  const pane = await writingTurn(t, cwd, 'add a page', [['src/page.txt', 'hello page']]);

  await t.step('Open Review from the sidebar');
  await openDeskWithPane(t, pane);

  await t.step('The desk lists the turn with its sentence, the agent, the file count and "No flags"');
  const row = await rowText(t, pane);
  t.expect.includes(row, 'Nimbus edited 1 file (+1 −0), now idle.');
  t.expect.includes(row, 'No flags');
  t.expect.includes(row, '1 file +1 −0');
  const head = await deskHead(t);
  t.expect.match(head, /Review · \d+/);
  t.expect.match(head, /Low risk [1-9]/);

  await t.step('Close the desk');
  await t.click('exact=Close');
  await t.expect.notVisible('css=.ck-rd');
}
