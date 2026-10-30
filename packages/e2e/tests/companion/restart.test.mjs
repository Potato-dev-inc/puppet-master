// More > Restart starts the companion's agent again in the same folder.
import { noCompanions, makeCompanion, openCompanion, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.restart',
  title: 'Restart starts the companion\'s agent again with a new process',
  area: 'companion',
  tags: [],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);
  const before = (await t.panes()).find((p) => p.id === made.paneId);

  await t.step('Open the menu and press Restart');
  await t.click('aria=More');
  await t.click('exact=Restart');

  await t.step('A new process runs in the same pane and the companion is back');
  await t.expect.eventually(async () => {
    const now = (await t.panes()).find((p) => p.id === made.paneId);
    return now && now.pid !== before.pid && now.status === 'idle';
  }, { timeout: 30000, message: 'expected a new process for the companion pane' });
  await t.expect.eventually(async () => (await companionList(t))[0].state === 'idle', { timeout: 30000, message: 'expected it to be here again' });
  t.expect.equal((await companionList(t))[0].folder, made.folder, 'the same folder');
  await t.expect.visible('exact=Sleep');
});
