// The add button stays off until there is something to ask; Cancel adds nothing.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.checkin-needs-text',
  title: 'A check-in cannot be added without text, and Cancel adds nothing',
  area: 'companion',
  tags: [],
  edge: true,
};

const addDisabled = (t) => t.page.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Add').disabled`);

export const run = guarded(async (t) => {
  await noCompanions(t);
  await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);
  await t.click('exact=Add a check-in');
  await t.expect.text('What to ask');

  await t.step('With nothing written the Add button is off');
  t.expect.equal(await addDisabled(t), true);

  await t.step('Only spaces is nothing');
  await t.page.fill('css=.ck-companion-input--area', '     ');
  t.expect.equal(await addDisabled(t), true);

  await t.step('Some text switches it on');
  await t.page.fill('css=.ck-companion-input--area', 'ping');
  t.expect.equal(await addDisabled(t), false);

  await t.step('Cancel: the sheet closes and no check-in exists');
  await t.click('exact=Cancel', { within: 'css=.ck-companion-sheet' });
  await t.waitGone('What to ask');
  t.expect.equal((await companionList(t))[0].checkins.length, 0);
  await t.expect.text('No check-ins yet.');
});
