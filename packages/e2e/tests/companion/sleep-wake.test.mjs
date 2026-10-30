// Sleep closes the companion's pane and keeps its notes; Wake up starts it again.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.sleep-wake',
  title: 'Sleep puts the companion to sleep and Wake up brings it back',
  area: 'companion',
  tags: ['smoke'],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  const paneId = made.paneId;
  await t.page.reload();
  await openCompanion(t);
  t.expect.ok((await t.panes()).some((p) => p.id === paneId), 'its pane is running');

  await t.step('Press Sleep');
  await t.click('exact=Sleep');
  await t.expect.text('Sleeping. Wake me when you need me.');
  await t.expect.visible('exact=Wake up Mochi');
  await t.expect.eventually(async () => !(await t.panes()).some((p) => p.id === paneId), { message: 'expected its pane to close' });
  t.expect.equal((await companionList(t))[0].state, 'sleeping');
  await t.expect.visible('Asleep', { within: 'css=.ck-side' });

  await t.step('Press Wake up');
  await t.click('exact=Wake up Mochi');
  await t.waitFor('exact=Sleep');
  await t.expect.eventually(async () => (await companionList(t))[0].state === 'idle', { timeout: 30000, message: 'expected it to be here again' });
  t.expect.ok((await t.panes()).some((p) => p.id === paneId), 'its pane is back');
  await t.expect.text('Here. Ask me anything.');
});
