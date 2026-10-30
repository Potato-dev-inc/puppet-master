// A note longer than the file's cap cannot be saved: the counter turns on a warning and Save stays off.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, makeCompanion, openCompanion, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.memory-over-cap',
  title: 'A note over the size limit cannot be saved and Cancel throws it away',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);
  await t.click('exact=Its notes');
  await t.click('exact=Edit');

  await t.step('Paste a note that is far too long');
  const long = 'x'.repeat(3000);
  await t.page.fill('css=textarea.ck-companion-editor', long, { delay: 0 });
  await t.expect.text('3,000 / ');

  await t.step('Save is switched off');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Save').disabled`), true, 'Save is disabled');

  await t.step('Cancel throws the draft away; the file is untouched');
  await t.click('exact=Cancel', { within: 'css=.ck-companion-rail' });
  await t.expect.notVisible('exact=Save');
  t.expect.ok(!fs.readFileSync(path.join(made.folder, 'MEMORY.md'), 'utf8').includes('xxxx'), 'MEMORY.md is unchanged');
});
