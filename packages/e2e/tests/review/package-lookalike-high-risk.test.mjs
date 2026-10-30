// Package check: an install command with a look-alike name raises the approval card to High risk, offline.
import { claudeAtApproval, lookupsOff } from './_area.mjs';

export const meta = {
  id: 'review.package-lookalike-high-risk',
  title: 'An install of a look-alike package name shows a High risk card with a Package check row',
  area: 'review',
  tags: ['package-check', 'smoke'],
};

export async function run(t) {
  await t.step('Registry lookups are off, so this test stays on this computer');
  await lookupsOff(t);

  await t.step('An agent asks to run "npm install lodahs"');
  const pane = await claudeAtApproval(t, 'npm install lodahs');

  await t.step('Open the Inbox: the card is High risk and names the look-alike');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Install a package with a look-alike name');
  await t.expect.text('High risk');
  await t.expect.text('lodahs looks like the popular package lodash (2 letters swapped). Check the name before you allow it.');

  await t.step('The Package check block has one row with the offline fact');
  await t.expect.text('Package check');
  await t.expect.text('Look closer · 1 of 1');
  await t.expect.text('Looks like lodash (2 letters swapped)');
  await t.expect.noText('downloads a week');

  await t.step('Package check only informs: the agent is still waiting for you');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'approval_answer').length, 0);
  t.expect.ok(pane.id, 'pane');
}
