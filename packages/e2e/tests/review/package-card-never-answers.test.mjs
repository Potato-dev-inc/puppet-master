// Package check: it never answers or blocks. The person's click answers the agent, and Deny reaches it.
import { claudeAtApproval, lookupsOff } from './_area.mjs';

export const meta = {
  id: 'review.package-card-never-answers',
  title: 'The look-alike card waits for you; Deny sends the refusal to the agent',
  area: 'review',
  tags: ['package-check'],
  edge: true,
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  await lookupsOff(t);
  const pane = await claudeAtApproval(t, 'npm install lodahs', { deny: 'ok, not installing lodahs' });

  await t.step('The card is up and the agent is waiting');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Install a package with a look-alike name');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'approval_answer').length, 0, 'nothing answered by itself');

  await t.step('Press Deny');
  await t.click('exact=Deny');

  await t.step('The agent was refused and carries on; the card is gone');
  const answer = await t.fake.waitEvent('approval_answer');
  t.expect.ok(answer, 'the agent got an answer');
  await pane.waitScreen('ok, not installing lodahs');
  await t.expect.noText('Install a package with a look-alike name');
}
