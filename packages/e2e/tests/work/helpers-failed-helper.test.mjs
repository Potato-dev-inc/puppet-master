// Helpers: a helper that crashes. The agent's Chat row says failed, and the helper shows up under "Needs you" with whose helper it is.
import { calm, S, gitProject, orchestrator, spawnHelperCmd, chatHelperRows, openChat, groupOfRow, helpersOf, waitStatus } from './_area.mjs';

export const meta = {
  id: 'work.helpers-failed-helper',
  title: 'A helper that crashes shows "failed" in the agent\'s Chat and is listed under Needs you as that agent\'s helper',
  area: 'work',
  tags: ['work', 'helpers', 'crash'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const helperDir = gitProject(t, 'failing', { 'README.md': 'x\n' });
  const orch = await orchestrator(t, {
    commands: [spawnHelperCmd(t, { cwd: helperDir, message: 'risky errand' })],
    helperTurns: [{ on: 'risky errand', steps: [S.think(1500), S.crash('helper panicked: out of memory', 101)] }],
    holdMs: 25000,
  });
  await orch.write('start helpers');
  await openChat(t);

  await t.step('The helper starts and crashes');
  await t.expect.eventually(async () => (await helpersOf(t, orch)).length === 1, { timeout: 40000, message: 'the helper exists' });
  const [helper] = await helpersOf(t, orch);
  await t.expect.eventually(async () => (await t.panes()).find((p) => p.id === helper.id)?.status === 'error', { timeout: 30000, message: 'the helper pane is in error' });

  await t.step('The agent\'s Chat row says failed');
  await t.expect.eventually(async () => (await chatHelperRows(t))[0]?.status === 'failed', { message: async () => `row: ${JSON.stringify(await chatHelperRows(t))}` });
  t.expect.includes((await chatHelperRows(t))[0].text, 'failed');

  await t.step('The sidebar lists the helper under Needs you and says whose it is');
  t.expect.equal(await groupOfRow(t, (await t.panes()).find((p) => p.id === helper.id).title), 'Needs you');
  await t.expect.text('Helper of start helpers');
  t.expect.equal((await orch.info()).status, 'running', 'the agent itself keeps working');
  void waitStatus;
}
