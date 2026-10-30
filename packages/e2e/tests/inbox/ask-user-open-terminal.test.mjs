// A question that names its pane has an Open terminal button that goes to that pane.
import { askUser, openInbox } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.ask-user-open-terminal',
  title: 'Open terminal on a question goes to the agent that asked',
  area: 'inbox',
  tags: ['inbox', 'ask-user'],
  edge: true,
};

export async function run(t) {
  const pane = await t.spawnPane('nimbus');
  await askUser(t, { question: 'Ready to start?', choices: [{ id: 'go', label: 'Go' }], pane_id: pane.id });
  await openInbox(t);
  await t.expect.text('Ready to start?');

  await t.step('Click Open terminal');
  await t.click('exact=Open terminal');

  await t.step('The pane of that agent opens');
  await t.expect.text('NIMBUS 0.4.2');
  await t.expect.visible('exact=Chat');
  await t.expect.notVisible('css=.ck-inbox');
}
