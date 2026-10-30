// The relay address typed in Settings is kept, and the Go live sheet then stops asking for it. (A room cannot start
// without a real relay on a network, so this stops at the sheet.)
import { openGoLive, guarded, openGeneralSettings, reveal, invoke, resetLiveRoom } from '../companion/_area.mjs';

export const meta = {
  id: 'live.relay-setting-saved',
  title: 'A relay address typed in Settings is saved and Go live stops asking for it',
  area: 'live',
  tags: ['needs-external'], // a relay that is up needs a network; here the address is a placeholder on this computer
};

export const run = guarded(async (t) => {
  await resetLiveRoom(t);
  await t.spawnPane('nimbus');
  await t.step('Open Settings > General > Live Room');
  await openGeneralSettings(t);
  await reveal(t, '[aria-label="Relay address"]');

  await t.step('Type a relay address');
  await t.expect.eventually(async () => {
    await t.page.fill('aria=Relay address', 'https://relay.invalid');
    await t.press('Tab');
    return (await invoke(t, 'live_room_config_get')).ok?.relay === 'https://relay.invalid';
  }, { message: 'expected the relay address to be saved', timeout: 20000 });

  await t.step('Go live no longer asks for a relay address and can be pressed');
  await t.page.reload();
  await openGoLive(t);
  await t.expect.noText('Live Room needs a relay address.');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('.ck-sheet--live .ck-btn--primary')].find(b => b.textContent === 'Go live').disabled`), false, 'Go live can be pressed');

  await t.press('Escape');
  await resetLiveRoom(t);
});
