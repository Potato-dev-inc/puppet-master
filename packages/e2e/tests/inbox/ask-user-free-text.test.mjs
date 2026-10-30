// A question that allows free text: type the answer (with Unicode) and send it.
import { askUser, openInbox, questionResult } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.ask-user-free-text',
  title: 'A free-text question takes a typed answer',
  area: 'inbox',
  tags: ['inbox', 'ask-user'],
};

const ANSWER = 'Nuage d’été 🌤';

export async function run(t) {
  const id = await askUser(t, { question: 'What should the release be called?', allow_free_text: true });
  await openInbox(t);
  await t.expect.text('What should the release be called?');
  await t.expect.visible('aria=Your answer');

  await t.step('Send is off for an empty answer');
  t.expect.ok(await t.page.eval(`document.querySelector('.ib-field button[type=submit]').disabled`), 'Send is off');

  await t.step('Type an answer with Unicode and send it');
  await t.page.type(ANSWER, { into: 'aria=Your answer' });
  await t.click('exact=Send', { within: 'css=.ib-field' });

  await t.step('The agent has the exact text');
  await t.expect.noText('What should the release be called?');
  const result = await questionResult(t, id);
  t.expect.equal([result.resolved, result.text], [true, ANSWER]);
}
