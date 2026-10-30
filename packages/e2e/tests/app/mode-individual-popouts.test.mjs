// Orchestration keeps panes in the cockpit; Individual gives every pane its own window. Switching back closes them.
import { paneWindows, paneWindowText, calm, restoreOrchestration } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'app.mode-individual-popouts',
  title: 'Individual mode opens one window per pane and Orchestration takes them back',
  area: 'app',
  tags: ['smoke', 'mode'],
};

async function body(t) {
  const a = await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  const b = await t.spawnPane('nimbus', { cwd: t.project('beta') });

  await t.step('Orchestration: the panes are in the cockpit, no pane windows');
  t.expect.equal((await paneWindows(t)).length, 0, 'no pane windows yet');

  await t.step('Switch to Individual');
  await t.click('exact=Individual');
  await t.expect.text('Individual mode');
  await t.expect.text('Panes are in their own windows.');
  await t.expect.visible('Switch to Orchestration');

  await t.step('Each pane has its own window');
  await t.expect.eventually(async () => (await paneWindows(t)).length === 2, { message: 'two pane windows', timeout: 15000 });
  const ids = (await paneWindows(t)).map((w) => w.paneId).sort();
  t.expect.equal(ids, [a.id, b.id].sort(), 'the windows belong to the two panes');

  await t.step('A pane window shows its agent');
  await t.expect.eventually(async () => /NIMBUS 0\.4\.2/.test((await paneWindowText(t, a.id)) || ''), { message: 'the window of the first pane shows the agent', timeout: 15000 });
  t.expect.includes(await paneWindowText(t, b.id), 'beta', 'the second window shows its own folder');

  await t.step('The sidebar still lists both panes');
  await t.expect.count(PANE_ROW, 2);

  await t.step('Switch back to Orchestration: the windows close');
  await t.click('exact=Orchestration');
  await t.expect.eventually(async () => (await paneWindows(t)).length === 0, { message: 'no pane windows', timeout: 15000 });
  await t.expect.noText('Panes are in their own windows.');

  await t.step('The panes are still running and open in the cockpit');
  t.expect.equal((await t.panes()).length, 2, 'both panes live on');
  await t.click('exact=Nimbus 1', { within: 'css=.ck-side' });
  await t.expect.text('NIMBUS 0.4.2');
}

export async function run(t) {
  await calm(t);
  try {
    await body(t);
  } finally {
    await restoreOrchestration(t); // the mode is a saved setting: leave it as it was
  }
}
