// Helpers: a helper that stops to ask for approval is never hidden by a fold. It is listed under Needs you, naming its parent, and the Inbox has its card.
import { titleOf, calm, S, gitProject, orchestrator, spawnHelperCmd, helperChips, helpersOf, groupOfRow, waitStatus } from './_area.mjs';

export const meta = {
  id: 'work.helpers-folded-helper-needing-you-stays-visible',
  title: 'A folded helper that asks for approval still shows under Needs you with its parent\'s name',
  area: 'work',
  tags: ['work', 'helpers', 'approval'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  const helperDir = gitProject(t, 'askerdir', { 'README.md': 'x\n' });
  const orch = await orchestrator(t, {
    commands: [spawnHelperCmd(t, { cwd: helperDir, message: 'ship it' })],
    helperTurns: [{ on: 'ship it', steps: [S.think(8000), S.approve({ kind: 'create', path: 'release.txt', allow: [S.say('published')], deny: [S.say('not published')] })] }],
    holdMs: 40000,
  });
  await orch.write('start helpers');
  await t.expect.eventually(async () => (await helpersOf(t, orch)).length === 1, { timeout: 40000, message: 'the helper exists' });
  await t.expect.eventually(async () => (await helperChips(t)).join() === '1 helper', { message: 'the chip' });

  await t.step('Fold the helpers away');
  await t.click('exact=1 helper', { within: 'css=.ck-side' });
  const [helper] = await helpersOf(t, orch);
  const title = await titleOf(t, helper.id);

  await t.step('The helper stops to ask: it appears under Needs you although it is folded');
  await waitStatus(t, { id: helper.id, info: async () => (await t.panes()).find((p) => p.id === helper.id) }, 'waiting_input', 40000);
  await t.expect.eventually(async () => (await groupOfRow(t, title)) === 'Needs you', { message: 'the helper row is under Needs you' });
  await t.expect.text('Helper of start helpers');

  await t.step('The Inbox has a card for it');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Answer the agent');
}
