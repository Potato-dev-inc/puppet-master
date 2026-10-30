// Scores on your code: the page reads counts only. It never shows what you asked, replies or folder paths.
import { basename } from 'node:path';
import { selectByTitle, openSidebarPage, runTurn, SLOW_REPLY, projectDir, counted } from './_area.mjs';

export const meta = {
  id: 'scores.shows-no-private-text',
  title: 'The Scores page never shows the prompt text or the folder path',
  area: 'scores',
  tags: ['scores'],
  edge: true,
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  t.fake.script(SLOW_REPLY);
  const cwd = projectDir(t, 'secret-folder-name');
  const pane = await t.spawnPane('nimbus', { cwd });

  await t.step('A turn with a private-looking request');
  await runTurn(t, pane, 'rotate the key sk-test-ABCDEF123456 for ana@example.invalid');

  await counted(t, cwd, 1);

  await t.step('Open Scores');
  await selectByTitle(t, 'rotate the key');
  await openSidebarPage(t, 'Scores');
  await t.expect.text('Not enough runs yet. 1 turn so far.');

  await t.step('Only the last folder name is shown; the request, the key and the full path are not');
  const text = await t.page.text();
  const scores = text.slice(text.indexOf('Scores on your code'));
  t.expect.includes(scores, `This project: ${basename(cwd)}`);
  for (const hidden of ['sk-test-ABCDEF123456', 'ana@example.invalid', 'rotate the key', 'pm-e2e', cwd, cwd.split('\\').join('/')]) {
    t.expect.ok(!scores.includes(hidden), `the Scores page must not show "${hidden}"`);
  }
}
