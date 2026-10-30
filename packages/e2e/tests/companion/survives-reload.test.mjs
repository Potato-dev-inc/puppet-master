// The companion is the app's, not the page's: reloading the cockpit window finds it again, with its name and state.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.survives-reload',
  title: 'Reloading the window finds the companion again, still awake',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t, { name: 'Hoot', avatar: 'hoot' });

  await t.step('Reload the window');
  await t.page.reload();

  await t.step('The sidebar still has it, awake, and the page opens on it');
  await t.expect.visible('exact=Hoot', { within: 'css=.ck-side' });
  await t.expect.visible('exact=Here', { within: 'css=.ck-side' });
  await openCompanion(t, 'Hoot');
  await t.expect.text('Here. Ask me anything.');

  await t.step('It is the same companion and the same process');
  const [c] = await companionList(t);
  t.expect.equal(c.id, made.id);
  const pane = (await t.panes()).find((p) => p.id === made.paneId);
  t.expect.ok(pane, 'its pane is still running');
});
