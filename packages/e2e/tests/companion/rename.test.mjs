// Click the name to rename it; Enter saves. The folder keeps its id.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.rename',
  title: 'Clicking the name renames the companion and its folder stays',
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Click the name and type a new one');
  await t.click('exact=Mochi', { within: 'css=.ck-companion-head' });
  await t.page.fill('aria=Companion name', 'Pip the Great');
  await t.press('Enter');

  await t.step('The page and the sidebar use the new name');
  await t.expect.visible('exact=Pip the Great', { within: 'css=.ck-side' });
  await t.expect.text('What Pip the Great knows');
  const [c] = await companionList(t);
  t.expect.equal(c.name, 'Pip the Great');
  t.expect.equal(c.id, made.id, 'the id and folder do not change');
  t.expect.equal(c.folder, made.folder);
});
