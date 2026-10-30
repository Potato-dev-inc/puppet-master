// Cancel on the archive question changes nothing.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.archive-cancel',
  title: 'Cancel on "Archive Mochi?" keeps the companion',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Ask to archive, then cancel');
  await t.click('aria=More');
  await t.click('exact=Archive');
  await t.expect.text('Archive Mochi?');
  await t.click('exact=Cancel');
  await t.waitGone('Archive Mochi?');

  await t.step('Nothing changed: still here with its pane');
  t.expect.equal((await companionList(t)).length, 1);
  t.expect.ok((await t.panes()).some((p) => p.id === made.paneId), 'its pane is still running');
  await t.expect.visible('exact=Sleep');

  await t.step('Escape closes the question too');
  await t.click('aria=More');
  await t.click('exact=Archive');
  await t.expect.text('Archive Mochi?');
  await t.press('Escape');
  await t.waitGone('Archive Mochi?');
  t.expect.equal((await companionList(t)).length, 1);
});
