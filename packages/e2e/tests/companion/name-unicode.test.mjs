// A name with no letters a folder name can use still works: the id falls back to "companion", and a second one gets a number.
import path from 'node:path';
import { noCompanions, makeCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.name-unicode',
  title: 'Companions with non-Latin names get their own folders and do not collide',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);

  await t.step('Make a companion called 日本語');
  const first = await makeCompanion(t, { name: '日本語', avatar: 'kit' });
  await t.step('Make a second one called ÅÄÖ');
  const second = await makeCompanion(t, { name: 'ÅÄÖ', avatar: 'hoot' });

  await t.step('Both names show in the sidebar');
  await t.page.reload();
  await t.expect.visible('exact=日本語', { within: 'css=.ck-side' });
  await t.expect.visible('exact=ÅÄÖ', { within: 'css=.ck-side' });

  await t.step('They have different ids and different folders');
  t.expect.equal(first.id, 'companion');
  t.expect.equal(second.id, 'companion-2');
  const list = await companionList(t);
  t.expect.equal(new Set(list.map((c) => path.normalize(c.folder).toLowerCase())).size, 2, 'two folders');
});
