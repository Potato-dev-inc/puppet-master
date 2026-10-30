// "What Mochi knows": Edit, type, Save writes the file in the companion's folder.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, makeCompanion, openCompanion, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.memory-edit',
  title: "Editing \"About you\" and pressing Save writes USER.md in the companion's folder",
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Open the About you tab and press Edit');
  await t.click('exact=About you');
  await t.expect.text('0 / ');
  await t.click('exact=Edit');

  await t.step('Type something with accents and symbols');
  const note = 'I like tea, not coffee. Zoë calls me "Ren" (日本語) & uses <b>bold</b>.';
  await t.page.fill('css=textarea.ck-companion-editor', note);
  await t.expect.text(`${Array.from(note).length} / `);

  await t.step('Save');
  await t.click('exact=Save');
  await t.expect.notVisible('exact=Save');

  await t.step('The file in its folder has the text');
  await t.expect.eventually(() => fs.readFileSync(path.join(made.folder, 'USER.md'), 'utf8').includes(note), { message: 'expected USER.md to hold the note' });
  await t.expect.visible('exact=Edit');
});
