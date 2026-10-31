// Scores on your code: from five turns the numbers show as "x of y" with a small "early" tag.
import { basename } from 'node:path';
import { selectByTitle, openSidebarPage, runTurn, SLOW_REPLY, projectDir, counted } from './_area.mjs';

export const meta = {
  id: 'scores.five-turns-early',
  title: 'After five turns the row shows "5 of 5" tagged early, not a percent',
  area: 'scores',
  tags: ['scores'],
  timeout: 120000,
};

export async function run(t) {
  t.fake.script(SLOW_REPLY);
  const cwd = projectDir(t, 'site');
  const pane = await t.spawnPane('nimbus', { cwd });

  await t.step('The agent finishes five turns');
  for (let i = 1; i <= 5; i++) await runTurn(t, pane, `task number ${i}`);

  await counted(t, cwd, 5);

  await t.step('Open Scores');
  await selectByTitle(t, 'task number 1');
  await openSidebarPage(t, 'Scores');
  await t.expect.text(`This project: ${basename(cwd)}`);

  await t.step('Turns shows 5, Ended OK shows 5 of 5 with the early tag, and there is no percent');
  await t.expect.eventually(async () => /Nimbus[\s\S]*\b5\b[\s\S]*5 of 5/.test(await t.page.text()), { message: 'a row with 5 turns and "5 of 5"' });
  await t.expect.visible('exact=early');
  await t.expect.noText('100%');
  await t.expect.noText('Not enough runs yet');
}
