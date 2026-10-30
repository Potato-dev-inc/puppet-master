// If the person closes the companion's terminal, it goes to sleep (it is not started again behind their back).
import { noCompanions, makeCompanion, openCompanion, companionList, invoke, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.terminal-closed-sleeps',
  title: "Closing the companion's terminal puts it to sleep instead of restarting it",
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const made = await makeCompanion(t);
  await t.page.reload();
  await openCompanion(t);

  await t.step('Close its terminal from outside, like closing the window');
  await t.bridge.del(`/panes/${made.paneId}`);

  await t.step('It goes to sleep and says so');
  await t.expect.text('Sleeping. Wake me when you need me.');
  await t.expect.eventually(async () => (await companionList(t))[0].state === 'sleeping', { message: 'expected the companion to sleep' });
  const feed = (await invoke(t, 'companion_feed', { id: made.id, sinceMs: 0 })).ok;
  t.expect.ok(feed.some((f) => f.kind === 'slept' && f.text.includes('closed its terminal')), 'the feed says you closed its terminal');

  await t.step('It stays asleep: no new pane appears');
  t.expect.equal((await t.panes()).filter((p) => p.id === made.paneId).length, 0);
  await t.expect.visible('exact=Wake up Mochi');
});
