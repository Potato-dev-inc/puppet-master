// Dismiss on a question: the card goes and the agent is told nothing was answered.
import { askUser, openInbox, pendingQuestions, questionResult } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.ask-user-dismiss',
  title: 'Dismiss removes a question and the agent hears that it was dismissed',
  area: 'inbox',
  tags: ['inbox', 'ask-user'],
  edge: true,
};

export async function run(t) {
  const id = await askUser(t, { question: 'Delete the old branches?', choices: [{ id: 'yes', label: 'Yes, delete' }, { id: 'no', label: 'No, keep' }] });
  await openInbox(t);
  await t.expect.text('Delete the old branches?');

  await t.step('Dismiss it');
  await t.click('exact=Dismiss');

  await t.step('The card is gone; the agent was told it was dismissed');
  await t.expect.noText('Delete the old branches?');
  const result = await questionResult(t, id);
  t.expect.equal(result.resolved, true);
  t.expect.ok(!result.choice_id && !result.text, `no answer was invented: ${JSON.stringify(result)}`);
  t.expect.equal((await pendingQuestions(t)).length, 0);
}
