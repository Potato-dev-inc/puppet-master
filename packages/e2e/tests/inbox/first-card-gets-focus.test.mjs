// The Inbox says it works without the mouse: the first card should take the keyboard when the Inbox opens with a card.
import { askUser, openInbox } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.first-card-gets-focus',
  title: 'The first card of the Inbox has the keyboard when the Inbox opens',
  area: 'inbox',
  tags: ['inbox', 'keyboard', 'known-bug'],
  edge: true,
  // Bug: the code (Inbox.tsx, "when the Inbox first has a card, focus the first one") promises it, but after opening the
  // Inbox from the sidebar the keyboard stays on the sidebar row, so J, K and 1 to 9 do nothing until the card is clicked.
  // Run it anyway with E2E_RUN_KNOWN_BUGS=1.
  skip: 'Opening the Inbox with a question waiting leaves the keyboard on the sidebar row; the first card is not focused',
};

export async function run(t) {
  if (meta.skip && !process.env.E2E_RUN_KNOWN_BUGS) t.skip(meta.skip);
  await askUser(t, { question: 'Run the slow tests too?', choices: [{ id: 'all', label: 'Run all' }, { id: 'fast', label: 'Fast only' }] });
  await openInbox(t);
  await t.expect.text('Run the slow tests too?');

  await t.step('The keyboard is on the card');
  await t.expect.eventually(() => t.page.eval(`!!document.activeElement.closest('[data-ib-card]')`), { message: 'the first card has the focus' });
}
