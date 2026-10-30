// Edge case: the mode is a saved setting. A reloaded cockpit window comes back in the same mode.
import { calm, restoreOrchestration } from './_area.mjs';

export const meta = {
  id: 'app.mode-survives-reload',
  title: 'Reloading the cockpit window keeps Individual mode',
  area: 'app',
  tags: ['mode', 'persistence'],
  edge: true,
};

async function body(t) {
  await t.step('Switch to Individual');
  await t.click('exact=Individual');
  await t.expect.text('Panes are in their own windows.');

  await t.step('Reload the window');
  await t.page.reload();
  await t.expect.text('Puppet Master');

  await t.step('It is still Individual mode');
  await t.expect.text('Panes are in their own windows.');
  await t.expect.text('Switch to Orchestration');

  await t.step('Switch back');
  await t.click('exact=Switch to Orchestration');
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
