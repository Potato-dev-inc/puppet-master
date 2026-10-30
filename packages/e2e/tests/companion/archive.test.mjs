// More > Archive asks first; the companion leaves the list and its pane closes, but its files stay in its folder.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.archive',
  title: 'Archive removes the companion from the list and keeps its files',
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Open the menu and press Archive');
  await t.click('aria=More');
  await t.click('exact=Archive');
  await t.expect.text('Archive Mochi?');
  await t.expect.text('It stops and leaves the list. Its files stay in its folder.');

  await t.step('Confirm');
  await t.click('exact=Archive', { within: 'css=.ck-companion-sheet' });

  await t.step('The companion is gone from the sidebar and its pane closed');
  await t.expect.visible('Make a companion', { within: 'css=.ck-side' });
  t.expect.equal((await companionList(t)).length, 0, 'no companion in the list');
  await t.expect.eventually(async () => !(await t.panes()).some((p) => p.id === made.paneId), { message: 'expected its pane to close' });

  await t.step('Its notes are still in its folder');
  t.expect.ok(fs.existsSync(path.join(made.folder, 'SOUL.md')), 'SOUL.md is still there');
});
