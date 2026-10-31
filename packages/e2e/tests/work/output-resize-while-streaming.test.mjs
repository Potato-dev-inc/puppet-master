// Output stress: the window is resized again and again while a reply streams in. When it ends, every row is on the screen once, in order.
import { calm, S, oneTurn, openPane, waitStatus, resizeWindow, rowNumbers, isCleanRun, streamSteps } from './_area.mjs';

export const meta = {
  id: 'work.output-resize-while-streaming',
  title: 'Resizing the window while a reply streams leaves every row once, in order, with no repeats',
  area: 'work',
  tags: ['work', 'output', 'resize'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await calm(t);
  t.fake.script(oneTurn([S.think(300), ...streamSteps(60, 'stream', 220), S.say('stream finished')]));
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await t.expect.eventually(async () => (await pane.info())?.cols !== 120, { message: 'the pane was fitted to the cockpit', timeout: 15000 });

  try {
    await t.step('A long reply starts streaming');
    await pane.write('stream it');
    await t.fake.waitEvent('prompt');
    await t.expect.text('stream row 3', { timeout: 20000 });

    await t.step('Make the window smaller and bigger again while it streams');
    for (const [w, h] of [[900, 560], [1280, 800], [1000, 640], [1280, 800], [920, 600]]) {
      await resizeWindow(t, w, h);
      await t.sleep(600);
    }
  } finally {
    await resizeWindow(t, 1280, 800).catch(() => {});
  }

  await t.step('When the reply ends, the screen holds last rows once each, in order, up to row 60');
  await waitStatus(t, pane, 'idle', 60000);
  await t.expect.text('stream finished');
  const nums = rowNumbers(await pane.screen());
  t.expect.ok(isCleanRun(nums, 60), `rows seen: ${JSON.stringify(nums)}`);

  await t.step('The agent was told the same grid the app has');
  await t.expect.eventually(async () => {
    const last = t.fake.events().filter((e) => e.ev === 'resize').pop();
    const now = await pane.info();
    return !!last && last.cols === now.cols && last.rows === now.rows;
  }, { message: 'the agent sees the app\'s grid', timeout: 15000 });
}
