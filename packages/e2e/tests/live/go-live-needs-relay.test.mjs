// Live Room offline: with no relay address the "Go live" sheet says so, cannot start, and points at the setting.
import { openGoLive, guarded, resetLiveRoom } from '../companion/_area.mjs';

export const meta = {
  id: 'live.go-live-needs-relay',
  title: 'Go live without a relay address says what is missing and cannot start',
  area: 'live',
  tags: ['smoke', 'needs-external'], // a real room needs a relay on a network; this tests everything before that
};

export const run = guarded(async (t) => {
  await resetLiveRoom(t); // an earlier test may have left a relay address
  await t.spawnPane('nimbus');

  await t.step("Open the pane's chat menu and choose Go live");
  await openGoLive(t);

  await t.step('The sheet lists the pane, the length and the viewer limit');
  await t.expect.text('Panes');
  await t.expect.text('Ends after');
  await t.expect.text('Viewers');
  await t.expect.text('Viewers see the chat and what each agent is doing.');

  await t.step('It says a relay address is needed, and Go live is off');
  await t.expect.text('Live Room needs a relay address.');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('.ck-sheet--live .ck-btn--primary')].find(b => b.textContent === 'Go live').disabled`), true, 'Go live is disabled');

  await t.step('The button leads to the Live Room settings');
  await t.click('exact=Open Live Room settings');
  await t.expect.text('Rooms are end-to-end encrypted.');
  await t.expect.visible('Relay address');
});
