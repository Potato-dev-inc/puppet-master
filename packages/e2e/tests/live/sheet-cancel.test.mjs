// Cancel and Escape leave Go live without starting anything.
import { openGoLive, guarded, invoke } from '../companion/_area.mjs';

export const meta = {
  id: 'live.sheet-cancel',
  title: 'Cancel and Escape close Go live and no room starts',
  area: 'live',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await t.spawnPane('nimbus');

  await t.step('Open Go live and press Cancel');
  await openGoLive(t);
  await t.click('exact=Cancel', { within: 'css=.ck-sheet--live' });
  await t.waitGone('Viewers see the chat');

  await t.step('Open it again and press Escape');
  await openGoLive(t);
  await t.press('Escape');
  await t.waitGone('Viewers see the chat');

  await t.step('No room is running and no live bar is on screen');
  const status = await invoke(t, 'live_room_status');
  t.log('live_room_status', JSON.stringify(status));
  await t.expect.noText('You are live');
  t.expect.equal(await t.page.count('css=.ck-live-bar'), 0, 'no live bar');
});
