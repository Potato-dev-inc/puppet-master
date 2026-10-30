// Edge case: Remove asks first, moves the files aside (nothing is deleted) and the Undo button puts the agent back.
import { calm, clickControl, openSettings, writeDefinition, definitionToml } from './_area.mjs';

export const meta = {
  id: 'settings.agent-definition-remove-undo',
  title: 'Removing an agent definition asks first, can be cancelled and undone',
  area: 'settings',
  tags: ['agents', 'definitions', 'undo'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const remove = writeDefinition(t, 'vega.toml', definitionToml('vega', 'Vega'));
  try {
    await openSettings(t, 'Agents');
    await clickControl(t, 'exact=Reload');
    await t.expect.text('Needs approval');

    await t.step('Remove: a sheet explains what happens, Cancel keeps the agent');
    await clickControl(t, 'aria=Remove Vega');
    await t.expect.text('Remove Vega?');
    await t.expect.text('Nothing is deleted.');
    await t.click('exact=Cancel');
    await t.expect.noText('Remove Vega?');
    await t.expect.text('Needs approval');

    await t.step('Remove for real');
    await clickControl(t, 'aria=Remove Vega');
    await t.click('exact=Remove', { within: 'css=[role=dialog]' });
    await t.expect.text('Removed Vega.');
    await t.expect.noText('Needs approval');

    await t.step('Undo brings it back');
    await t.click('exact=Undo');
    await t.expect.text('Needs approval');
    await t.expect.noText('Removed Vega.');
  } finally {
    remove();
    await t.bridge.post('/agents/definitions/reload', {}).catch(() => {});
  }
}
