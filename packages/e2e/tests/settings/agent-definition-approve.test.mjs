// A new definition stays off until the person reviews and approves it. After the approval the agent is in the + menu.
import { calm, clickControl, openSettings, writeDefinition, ORBIT_TOML } from './_area.mjs';

export const meta = {
  id: 'settings.agent-definition-approve',
  title: 'A new agent definition waits for approval and works after Review and approve',
  area: 'settings',
  tags: ['agents', 'definitions'],
};

export async function run(t) {
  await calm(t);
  const remove = writeDefinition(t, 'orbit.toml', ORBIT_TOML);
  try {
    await openSettings(t, 'Agents');

    await t.step('Reload: the new definition appears and is not active');
    await clickControl(t, 'exact=Reload');
    await t.expect.text('orbit');
    await t.expect.text('Needs approval');
    await t.expect.visible('exact=Review and approve');

    await t.step('The + menu does not offer it yet');
    await t.click('aria=New agent');
    await t.waitFor('css=.ck-menu');
    await t.expect.notVisible('exact=Orbit', { within: 'css=.ck-menu' });
    await t.click('aria=New agent'); // the button closes the menu again (Esc would also leave Settings)

    await t.step('Review and approve: the dialog shows the command that would run');
    await clickControl(t, 'exact=Review and approve');
    await t.waitFor('css=[role=dialog]');
    await t.expect.text('Visible pane command');
    await t.expect.text('nimbus');

    await t.step('Approve');
    await t.click('exact=Approve', { within: 'css=[role=dialog]' });
    await t.expect.eventually(async () => !(await t.page.has('exact=Review and approve')), { message: 'the Review and approve button is gone' });
    await t.expect.text('Active');

    await t.step('Now the + menu offers Orbit');
    await t.click('aria=New agent');
    await t.expect.visible('exact=Orbit', { within: 'css=.ck-menu' });
    await t.click('aria=New agent'); // the button closes the menu again (Esc would also leave Settings)
  } finally {
    remove();
    await t.bridge.post('/agents/definitions/reload', {}).catch(() => {});
  }
}
