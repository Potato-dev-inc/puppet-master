// Scores on your code: a project where no agent has run yet has nothing to compare, and the page says so.
import { basename } from 'node:path';
import { openSidebarPage, projectDir, selectByTitle } from './_area.mjs';

export const meta = {
  id: 'scores.empty-state',
  title: 'In a project where no agent has run a turn, Scores says there is nothing to show',
  area: 'scores',
  tags: ['scores'],
  edge: true,
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  await t.step('An agent is started in a new folder but is given no task');
  const cwd = projectDir(t, 'untouched');
  await t.spawnPane('nimbus', { cwd });
  await t.click('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', { nth: 0 });

  await t.step('Open Scores from the sidebar: it opens on this project');
  await openSidebarPage(t, 'Scores');
  await t.expect.text('Scores on your code');
  await t.expect.text('Which agent works best here, from your own history.');
  await t.expect.text(`This project: ${basename(cwd)}`);

  await t.step('The page says there are no turns here yet and shows no row');
  await t.expect.text('No turns here yet. Run an agent in this project, then come back.');
  await t.expect.noText('Not enough runs yet');

  await t.step('The reading help is still there');
  await t.click('How to read this');
  await t.expect.text('Agents never see this page.');
}
