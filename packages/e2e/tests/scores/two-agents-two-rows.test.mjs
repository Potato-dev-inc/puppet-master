// Scores on your code: each agent is its own row, sorted by turns, never by a score.
import { basename } from 'node:path';
import { installAgent, selectByTitle, openSidebarPage, runTurn, SLOW_REPLY, projectDir, counted } from './_area.mjs';

export const meta = {
  id: 'scores.two-agents-two-rows',
  title: 'Two different agents in one project get one row each, the busier one first',
  area: 'scores',
  tags: ['scores'],
  timeout: 90000,
};

export async function run(t) {
  await installAgent(t, { id: 'ember', label: 'Ember', transcript: false });
  t.fake.script(SLOW_REPLY);
  const cwd = projectDir(t, 'shared');
  const nimbus = await t.spawnPane('nimbus', { cwd });
  const ember = await t.spawnPane('ember', { cwd });

  await t.step('Nimbus finishes two turns and Ember finishes one');
  await runTurn(t, nimbus, 'nimbus one');
  await runTurn(t, nimbus, 'nimbus two');
  await runTurn(t, ember, 'ember one');

  await counted(t, cwd, 3);

  await t.step('Open Scores for this project');
  await selectByTitle(t, 'nimbus one');
  await openSidebarPage(t, 'Scores');
  await t.expect.text(`This project: ${basename(cwd)}`);

  await t.step('Both agents have a row; Nimbus (2 turns) is above Ember (1 turn)');
  await t.expect.text('Not enough runs yet. 2 turns so far.');
  await t.expect.text('Not enough runs yet. 1 turn so far.');
  const order = await t.page.eval(`[...document.querySelectorAll('.ck-scores-table tbody th .ck-scores-agent')].map(e => e.textContent)`);
  t.expect.equal(order, ['Nimbus', 'Ember']);
}
