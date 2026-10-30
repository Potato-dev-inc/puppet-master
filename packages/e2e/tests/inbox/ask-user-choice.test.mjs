// An agent asks the person a question with choices (the ask_user tool). It shows in the Inbox; a click on a choice answers it and the agent gets the answer.
import { askUser, openInbox, pendingQuestions, questionResult } from '../chat/_area.mjs';

export const meta = {
  id: 'inbox.ask-user-choice',
  title: 'A question with choices from an agent is answered with one click',
  area: 'inbox',
  tags: ['inbox', 'ask-user', 'smoke'],
};

export async function run(t) {
  const pane = await t.spawnPane('nimbus');

  await t.step('The agent asks which database to use');
  const id = await askUser(t, {
    question: 'Which database should I use?',
    choices: [{ id: 'pg', label: 'Postgres' }, { id: 'lite', label: 'SQLite' }],
    context: 'for the new service',
    pane_id: pane.id,
  });
  await openInbox(t);
  await t.expect.text('One thing needs you');
  await t.expect.text('Which database should I use?');
  await t.expect.text('for the new service');
  await t.expect.visible('exact=Postgres');
  await t.expect.visible('exact=SQLite');
  t.expect.equal((await questionResult(t, id)).resolved, false, 'the agent is still waiting');

  await t.step('Click Postgres');
  await t.click('exact=Postgres');

  await t.step('The card is gone and the agent has the answer');
  await t.expect.noText('Which database should I use?');
  const result = await questionResult(t, id);
  t.expect.equal([result.resolved, result.choice_id, result.answered_by], [true, 'pg', 'desktop']);
  t.expect.equal((await pendingQuestions(t)).length, 0, 'no question is waiting');
}
