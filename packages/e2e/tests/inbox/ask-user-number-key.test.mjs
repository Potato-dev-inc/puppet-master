// Keyboard only: with the Inbox open, the key 2 picks the second choice of the card in front.
import { askUser, openInbox, questionResult } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.ask-user-number-key',
  title: 'The key 2 answers the second choice of the card in front',
  area: 'inbox',
  tags: ['inbox', 'ask-user', 'keyboard'],
  edge: true,
};

export async function run(t) {
  const id = await askUser(t, { question: 'Run the slow tests too?', choices: [{ id: 'all', label: 'Run all' }, { id: 'fast', label: 'Fast only' }] });
  await openInbox(t);
  await t.expect.text('Run the slow tests too?');
  await t.expect.visible('aria=Keyboard shortcuts');

  await t.step('Put the keyboard on the card, then press 2');
  await t.click('exact=Run the slow tests too?');
  await t.press('2');

  await t.step('The second choice was sent');
  await t.expect.noText('Run the slow tests too?');
  t.expect.equal((await questionResult(t, id)).choice_id, 'fast');
}
