// Package check: a well-known package name is not flagged.
import { claudeAtApproval, lookupsOff } from './_area.mjs';

export const meta = {
  id: 'review.package-plain-name-no-flag',
  title: 'Installing a well-known package name is not called a look-alike',
  area: 'review',
  tags: ['package-check'],
  edge: true,
};

export async function run(t) {
  await lookupsOff(t);

  await t.step('An agent asks to run "npm install lodash"');
  await claudeAtApproval(t, 'npm install lodash');

  await t.step('The card is in the Inbox, with no look-alike warning');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.visible('exact=Allow once');
  await t.expect.noText('look-alike');
  await t.expect.noText('Look closer');
  await t.expect.noText('High risk');

  await t.step('The card still has its buttons, in the same place');
  await t.expect.visible('exact=Deny');
}
