// Scores on your code: one turn is a count, never a score.
import { basename } from 'node:path';
import { selectByTitle, openSidebarPage, runTurn, SLOW_REPLY, projectDir, counted } from './_area.mjs';

export const meta = {
  id: 'scores.one-turn-not-enough',
  title: 'After one turn the agent has a row that says there are not enough runs yet',
  area: 'scores',
  tags: ['scores', 'smoke'],
};

export async function run(t) {
  t.fake.script(SLOW_REPLY);
  const cwd = projectDir(t, 'site');
  const pane = await t.spawnPane('nimbus', { cwd });

  await t.step('The agent finishes one turn');
  await runTurn(t, pane, 'first task');

  await counted(t, cwd, 1);

  await t.step('Open Scores: the page is about this project');
  await selectByTitle(t, 'first task');
  await openSidebarPage(t, 'Scores');
  await t.expect.text(`This project: ${basename(cwd)}`);

  await t.step('One row for Nimbus says one turn is not enough to compare');
  await t.expect.text('Not enough runs yet. 1 turn so far.');
  await t.expect.visible('exact=Nimbus');
  await t.expect.noText('ahead');
}
