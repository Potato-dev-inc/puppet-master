// The pane menu's Reflow pressed in the middle of a streaming reply: the agent keeps working and the screen stays clean.
import { calm, S, oneTurn, openPane, waitStatus, rowNumbers, isCleanRun, streamSteps } from './_area.mjs';

export const meta = {
  id: 'work.menu-reflow-while-streaming',
  title: 'Reflow from the pane menu during a streaming reply keeps the agent working and every row once',
  area: 'work',
  tags: ['work', 'output', 'menu'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(300), ...streamSteps(50, 'flow', 220), S.say('flow finished')]));
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await pane.write('stream it');
  await t.fake.waitEvent('prompt');
  await t.expect.text('flow row 5', { timeout: 20000 });

  await t.step('Pane menu > Reflow, three times while the reply streams');
  for (let i = 0; i < 3; i++) {
    await t.page.rightClick('css=.ck-side .ck-row.is-active');
    await t.click('exact=Reflow');
    t.expect.equal((await pane.info()).status, 'running', 'the agent keeps working');
    await t.sleep(700);
  }

  await t.step('The reply finishes and the screen holds last rows once each, in order, up to row 50');
  await waitStatus(t, pane, 'idle', 60000);
  await t.expect.text('flow finished');
  const nums = rowNumbers(await pane.screen(), 'flow');
  t.expect.ok(isCleanRun(nums, 50), `rows seen: ${JSON.stringify(nums)}`);
}
