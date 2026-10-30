// Edge case: the window is made smaller. The terminal is fitted to the new size and the agent is told (the same grid on both sides).
import { calm, openPane, resizeWindow } from './_area.mjs';

export const meta = {
  id: 'panes.terminal-resize',
  title: 'Resizing the window re-fits the terminal and the agent gets the new size',
  area: 'panes',
  tags: ['terminal', 'resize'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await t.expect.text('Type a task...');

  // The pane is first started at 120x32; showing it in the cockpit fits it to the window.
  await t.expect.eventually(async () => (await pane.info())?.cols !== 120, { message: 'the pane was fitted to the cockpit', timeout: 15000 });
  const before = await pane.info();
  t.log('before', { cols: before.cols, rows: before.rows });

  try {
    await t.step('Make the window smaller');
    await resizeWindow(t, 900, 560);
    await t.expect.eventually(async () => {
      const now = await pane.info();
      return now.rows < before.rows && now.cols < before.cols;
    }, { message: 'the pane has fewer rows and columns', timeout: 15000 });

    await t.step('The agent was told the new size');
    await t.expect.eventually(async () => {
      const now = await pane.info();
      const last = t.fake.events().filter((e) => e.ev === 'resize').pop();
      return !!last && last.cols === now.cols && last.rows === now.rows;
    }, { message: 'the agent sees the same grid as the app', timeout: 15000 });
    await t.expect.text('Type a task...');
  } finally {
    await resizeWindow(t, 1280, 800).catch(() => {});
  }

  await t.step('Back to the big window: the grid grows again');
  await t.expect.eventually(async () => (await pane.info()).rows >= before.rows - 1, { message: 'the pane is tall again', timeout: 15000 });
}
