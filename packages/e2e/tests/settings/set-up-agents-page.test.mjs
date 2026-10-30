// "Set up agents" in the sidebar lists the agents of this computer with their state; Close goes back. (The Test buttons start a
// real agent program, so this test never presses them. Only the fake agent Nimbus is looked at.)
import { calm } from './_area.mjs';

export const meta = {
  id: 'settings.set-up-agents-page',
  title: 'Set up agents lists the agents on this computer and Close returns to the Inbox',
  area: 'settings',
  tags: ['agents'],
};

export async function run(t) {
  await calm(t);

  await t.step('Click Set up agents in the sidebar');
  await t.click('exact=Set up agents', { within: 'css=.ck-side' });
  await t.expect.text('Set up your agents');
  await t.expect.text('Here is which ones are on this computer and what to do about the rest.');

  await t.step('Nimbus is listed as installed');
  await t.expect.visible('exact=Nimbus');
  await t.expect.text('Installed.');
  await t.expect.visible('exact=Check again');
  await t.expect.visible('Browse more agents');

  await t.step('Close goes back to the Inbox');
  await t.click('exact=Close');
  await t.expect.noText('Here is which ones are on this computer');
  await t.expect.text('Nothing needs you');
}
