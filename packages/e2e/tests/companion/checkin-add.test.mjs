// "Add a check-in": a schedule and what to ask. It shows in the card in plain words and is saved with the companion.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.checkin-add',
  title: 'Adding a daily check-in shows it in the card in plain words',
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);
  await t.expect.text('No check-ins yet.');

  await t.step('Press Add a check-in');
  await t.click('exact=Add a check-in');
  await t.expect.text('What to ask');

  await t.step('Write what to ask (with symbols and a second language)');
  await t.page.fill('css=.ck-companion-input--area', 'Morning! What is on today? Réponds en français, s\'il te plaît.');
  await t.step('Add it');
  await t.click('exact=Add');

  await t.step('The card lists it with its schedule');
  await t.expect.text('Every day at 8:00');
  await t.expect.text('Morning! What is on today?');
  await t.expect.noText('No check-ins yet.');
  const [c] = await companionList(t);
  t.expect.equal(c.checkins.length, 1);
  t.expect.equal(c.checkins[0].when.kind, 'daily');
  t.expect.equal(c.checkins[0].enabled, true);
});
