// Collision Alert: "Open <agent>" goes to that agent's pane; "Ignore" hides the card and the chat row.
import { twoWriters, ALERT_TITLE } from './_area.mjs';

export const meta = {
  id: 'collision.ignore-and-open',
  title: 'Open goes to the other agent, Ignore hides the alert',
  area: 'collision',
  tags: ['collision'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await twoWriters(t);
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text(ALERT_TITLE, { timeout: 90000 });

  await t.step('Press "Open edit beta": that agent\'s pane opens');
  await t.click('exact=Open edit beta');
  await t.expect.visible('exact=Terminal');
  await t.expect.text('edit beta');
  await t.expect.noText(ALERT_TITLE);

  await t.step('Go back to the Inbox and press Ignore');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text(ALERT_TITLE);
  await t.click('exact=Ignore');

  await t.step('The card is gone, and so is the row above the chat');
  await t.expect.noText(ALERT_TITLE);
  await t.click('edit alpha', { within: 'css=.ck-side' });
  await t.click('exact=Chat');
  await t.waitFor('aria=Message to the agent');
  await t.expect.noText('is working in the same folder');
}
