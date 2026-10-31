// Helpers: clicking the "Helper:" row in the agent's Chat opens that helper's own pane, which says whose helper it is.
import { titleOf, calm, S, gitProject, orchestrator, startHelpers, spawnHelperCmd, chatHelperRows, openChat, helpersOf, PANE_TITLE } from './_area.mjs';

export const meta = {
  id: 'work.helpers-click-row-opens-helper',
  title: 'Clicking a Helper row in Chat opens the helper\'s pane',
  area: 'work',
  tags: ['work', 'helpers'],
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const helperDir = gitProject(t, 'openrow', { 'README.md': 'x\n' });
  const orch = await orchestrator(t, {
    commands: [spawnHelperCmd(t, { cwd: helperDir, message: 'inspect logs' })],
    helperTurns: [{ on: 'inspect logs', steps: [S.think(500), S.say('logs inspected')] }],
    holdMs: 10000,
  });
  await orch.write('start helpers');
  await openChat(t);
  await t.expect.eventually(async () => (await chatHelperRows(t)).length === 1, { timeout: 40000, message: 'the Helper row' });
  const [helper] = await helpersOf(t, orch);
  const title = await titleOf(t, helper.id);

  await t.step('Click the Helper row');
  await t.click('css=[data-testid=pane-helper-row] button');

  await t.step('The helper\'s pane is open: its title in the header, its own folder, and its own screen');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('.ck-pane-head .ck-title')?.innerText?.trim() || ''`)) === title, { message: `the header shows "${title}"` });
  await t.expect.text('Helper of start helpers');
  await t.click('exact=Terminal');
  await t.expect.text('logs inspected');
  void PANE_TITLE;
}
