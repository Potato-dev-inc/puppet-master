// Before any agent was ever started, the Inbox greets the person and offers the first step instead of "nothing needs you".
import { openInbox } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.first-run-welcome',
  title: 'The Inbox of a fresh install welcomes the person and offers to set up agents',
  area: 'inbox',
  tags: ['inbox'],
  edge: true,
};

export async function run(t) {
  // The page remembers that an agent was ever started (the harness started one to open the window): forget it.
  await t.page.eval(`localStorage.removeItem('pm.cockpit.everSpawned')`);
  await t.page.reload();
  await t.step('Open the Inbox before any agent ever ran');
  await openInbox(t);
  await t.expect.text('Welcome to Puppet Master');
  await t.expect.visible('exact=Set up your agents');
  await t.expect.visible('exact=Use Simple mode');
  await t.expect.noText('Nothing needs you');
}
