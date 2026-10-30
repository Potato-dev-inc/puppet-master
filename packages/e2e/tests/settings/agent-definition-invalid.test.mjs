// Edge case: a definition file with a mistake is listed as Invalid with the reason, cannot be approved, and does not
// disturb the agents that work. Fixing the file makes it a normal definition again.
import { calm, clickControl, openSettings, writeDefinition, definitionToml } from './_area.mjs';

export const meta = {
  id: 'settings.agent-definition-invalid',
  title: 'A broken definition file is shown as Invalid and the other agents keep working',
  area: 'settings',
  tags: ['agents', 'definitions'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  const remove = writeDefinition(t, 'comet.toml', 'schema_version = 1\n[agent\nid = "comet"\nthis is not toml at all\n');
  try {
    await openSettings(t, 'Agents');

    await t.step('Reload: the broken file is listed as Invalid');
    await clickControl(t, 'exact=Reload');
    await t.expect.text('Invalid');
    await t.expect.text('comet.toml');

    await t.step('It has no approve button and is not in the + menu');
    await t.expect.notVisible('exact=Review and approve');
    await t.click('aria=New agent');
    await t.waitFor('css=.ck-menu');
    await t.expect.notVisible('exact=Comet', { within: 'css=.ck-menu' });
    await t.click('aria=New agent'); // the button closes the menu again (Esc would also leave Settings)

    await t.step('Nimbus still works');
    const pane = await t.spawnPane('nimbus');
    t.expect.equal((await pane.info()).agent_type, 'nimbus', 'a Nimbus pane started');
    await pane.kill();

    await t.step('Fix the file: it is a normal definition that needs approval');
    writeDefinition(t, 'comet.toml', definitionToml('comet', 'Comet'));
    await clickControl(t, 'exact=Reload');
    await t.expect.text('Needs approval');
    await t.expect.noText('Invalid');
  } finally {
    remove();
    await t.bridge.post('/agents/definitions/reload', {}).catch(() => {});
  }
}
