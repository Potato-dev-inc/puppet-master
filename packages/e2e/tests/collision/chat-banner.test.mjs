// Collision Alert: the same warning appears as one row above the chat of each of the two agents.
import { twoWriters } from './_area.mjs';

export const meta = {
  id: 'collision.chat-banner',
  title: 'The chat of each agent shows a row saying the other agent works in the same folder',
  area: 'collision',
  tags: ['collision', 'flaky'],
  skip: 'FLAKY under load: the chat row for the second agent did not show within 90 s in the full video run (it passed in 3 other runs); not yet understood',
  timeout: 120000,
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  await twoWriters(t);

  await t.step('Open the Chat tab of the "edit alpha" agent');
  await t.click('edit alpha', { within: 'css=.ck-side' });
  await t.click('exact=Chat');
  await t.waitFor('aria=Message to the agent');

  await t.step('A row above the chat says the other agent works in the same folder');
  await t.expect.text('edit beta is working in the same folder', { timeout: 90000 });
  await t.expect.visible('exact=Open edit beta');

  await t.step('The other agent\'s chat says the same about the first one');
  await t.click('edit beta', { within: 'css=.ck-side' });
  await t.click('exact=Chat');
  await t.waitFor('aria=Message to the agent');
  await t.expect.text('edit alpha is working in the same folder');
}
