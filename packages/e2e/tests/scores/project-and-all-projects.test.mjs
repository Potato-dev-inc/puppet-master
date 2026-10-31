// Scores on your code: "This project" counts only that folder; "All projects" counts every folder.
import { basename } from 'node:path';
import { openSidebarPage, runTurn, SLOW_REPLY, selectByTitle, projectDir, counted } from './_area.mjs';

export const meta = {
  id: 'scores.project-and-all-projects',
  title: 'This project counts one folder, All projects counts every folder',
  area: 'scores',
  tags: ['scores'],
  timeout: 90000,
};

/** The turns of the Nimbus row as the page shows them (a full row or the "so far" sentence). 0 when there is no row. */
const nimbusTurns = (t) => t.page.eval(`(()=>{
  const row=[...document.querySelectorAll('.ck-scores-table tbody tr')].find(r=>r.querySelector('.ck-scores-agent')?.textContent==='Nimbus');
  if(!row) return 0;
  const m=row.textContent.match(/(\\d+) turns? so far/);
  return m?Number(m[1]):Number(row.querySelector('td.is-num').textContent);
})()`);

export async function run(t) {
  t.fake.script(SLOW_REPLY);
  const alpha = projectDir(t, 'alpha');
  const beta = projectDir(t, 'beta');

  await t.step('Before anything runs: how many turns does All projects count already?');
  await openSidebarPage(t, 'Scores');
  await t.click('exact=All projects');
  await t.expect.text('Scores on your code');
  const before = await nimbusTurns(t);
  await t.click('exact=Close');

  await t.step('Folder alpha has two turns, folder beta has one');
  const a = await t.spawnPane('nimbus', { cwd: alpha });
  const b = await t.spawnPane('nimbus', { cwd: beta });
  await runTurn(t, a, 'alpha work one');
  await runTurn(t, a, 'alpha work two');
  await runTurn(t, b, 'beta work');

  await counted(t, alpha, 2);
  await counted(t, beta, 1);

  await t.step('Select the alpha pane and open Scores: only alpha counts');
  await selectByTitle(t, 'alpha work one');
  await openSidebarPage(t, 'Scores');
  await t.expect.text(`This project: ${basename(alpha)}`);
  await t.expect.text('Not enough runs yet. 2 turns so far.');

  await t.step('Press All projects: the three new turns are added to the total');
  await t.click('exact=All projects');
  await t.expect.eventually(async () => (await nimbusTurns(t)) >= Math.max(3, before + 3), { message: `All projects should count at least ${Math.max(3, before + 3)} turns (it counted ${before} before)` });

  await t.step('Back to This project: two turns again');
  await t.click(`This project: ${basename(alpha)}`);
  await t.expect.text('Not enough runs yet. 2 turns so far.');
}
