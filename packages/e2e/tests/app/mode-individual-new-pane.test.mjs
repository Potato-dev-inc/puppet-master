// Edge case: in Individual mode a pane started later gets its own window at once, and a pane that ends loses its window.
import { paneWindows, calm, restoreOrchestration } from './_area.mjs';

export const meta = {
  id: 'app.mode-individual-new-pane',
  title: 'In Individual mode a new pane gets a window and a closed pane loses it',
  area: 'app',
  tags: ['mode'],
  edge: true,
};

async function body(t) {
  await t.step('Switch to Individual with no panes');
  await t.click('exact=Individual');
  await t.expect.text('Panes are in their own windows.');
  t.expect.equal((await paneWindows(t)).length, 0, 'no windows with no panes');

  await t.step('Start an agent: its own window opens');
  const pane = await t.spawnPane('nimbus');
  await t.expect.eventually(async () => (await paneWindows(t)).some((w) => w.paneId === pane.id), { message: 'the new pane has a window', timeout: 15000 });

  await t.step('Close the pane from outside: the window goes with it');
  await pane.kill();
  await t.expect.eventually(async () => (await paneWindows(t)).length === 0, { message: 'the window closed', timeout: 15000 });
  await t.expect.text('No agents running.');

  await t.step('Back to Orchestration');
  await t.click('exact=Orchestration');
  await t.expect.noText('Panes are in their own windows.');
}

export async function run(t) {
  await calm(t);
  try {
    await body(t);
  } finally {
    await restoreOrchestration(t); // the mode is a saved setting: leave it as it was
  }
}
