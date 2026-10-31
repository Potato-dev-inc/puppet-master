// The Inbox says it works without the mouse: the first card should take the keyboard when the Inbox opens with a card.
import { askUser, openInbox } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.first-card-gets-focus',
  title: 'The first card of the Inbox has the keyboard when the Inbox opens',
  area: 'inbox',
  tags: ['inbox', 'keyboard'],
  edge: true,
};

export async function run(t) {
  await askUser(t, { question: 'Run the slow tests too?', choices: [{ id: 'all', label: 'Run all' }, { id: 'fast', label: 'Fast only' }] });
  await openInbox(t);
  await t.expect.text('Run the slow tests too?');

  await t.step('The keyboard is on the card');
  await t.expect.eventually(() => t.page.eval(`!!document.activeElement.closest('[data-ib-card]')`), { message: 'the first card has the focus' });
}
