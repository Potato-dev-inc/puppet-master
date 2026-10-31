// Helpers: the agent's Chat shows each helper it started as a "Helper: ..." row whose status follows the helper (working, then done).
import { knownBug, titleOf, calm, S, gitProject, orchestrator, startHelpers, spawnHelperCmd, chatHelperRows, openChat, waitStatus, helpersOf } from './_area.mjs';

export const meta = {
  id: 'work.helpers-rows-in-chat',
  title: 'The agent\'s Chat lists its helper as a "Helper:" row that goes from working to done',
  area: 'work',
  tags: ['work', 'helpers', 'known-bug'],
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const helperDir = gitProject(t, 'chatrow', { 'README.md': 'x\n' });
  const orch = await orchestrator(t, {
    commands: [spawnHelperCmd(t, { cwd: helperDir, message: 'compile notes' })],
    helperTurns: [{ on: 'compile notes', steps: [S.think(30000), S.write('notes.txt', 'compiled\n'), S.say('notes compiled')] }],
    holdMs: 45000,
  });
  await orch.write('start helpers');
  await openChat(t);
  await t.fake.waitEvent((e) => e.ev === 'prompt' && e.text === 'start helpers');

  await t.step('Chat of the agent: a Helper row appears and says working');
  await t.expect.eventually(async () => (await chatHelperRows(t)).length === 1, { timeout: 40000, message: async () => `rows: ${JSON.stringify(await chatHelperRows(t))}` });
  const [helper] = await helpersOf(t, orch);
  const title = await titleOf(t, helper.id);
  await knownBug(t, 'a helper that the bridge reports as running (it works for 30 s) shows as "done" in the agent Chat and as idle in the sidebar when an earlier helper was started in the same app session', async () => {
    await t.expect.eventually(async () => (await chatHelperRows(t))[0]?.status === 'working', { timeout: 20000, message: async () => { const h = (await t.panes()).find((p) => p.id === helper.id); return `the row should say working: ${JSON.stringify(await chatHelperRows(t))}; the bridge says the helper is ${h?.status}`; } });
  });
  const row = (await chatHelperRows(t))[0];
  t.expect.includes(row.text, 'Helper:');
  t.expect.includes(row.text, title);
  t.expect.includes(row.text, 'working');

  await t.step('When the helper finishes the row says done');
  await t.expect.eventually(async () => (await chatHelperRows(t))[0]?.status === 'done', { timeout: 60000, message: async () => `the row is done: ${JSON.stringify(await chatHelperRows(t))}` });
  t.expect.includes((await chatHelperRows(t))[0].text, 'done');
  t.expect.equal((await chatHelperRows(t)).length, 1, 'still one row');
}
