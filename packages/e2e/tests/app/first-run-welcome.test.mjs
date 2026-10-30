// A fresh profile shows the welcome once; Skip setup closes it. (Every other test starts with the welcome kept off, see calm().)
export const meta = {
  id: 'app.first-run-welcome',
  title: 'A fresh profile shows the welcome and Skip setup closes it',
  area: 'app',
  tags: ['first-run'],
};

export async function run(t) {
  // A fresh profile has no "welcome finished" setting and this window has never seen a pane. Make it so (no app restart
  // needed: a restart can mix up the launcher when other apps run at the same time, see forgetBridgePort in _area.mjs).
  for (const p of await t.panes()) await t.bridge.del(`/panes/${encodeURIComponent(p.id)}`).catch(() => {});
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'no panes at the start' });
  await t.page.eval(`window.__TAURI_INTERNALS__.invoke('settings_patch', { patch: { onboarding_done: null, onboarding_step: null } }).then(() => true)`);
  await t.page.eval('localStorage.clear()');
  await t.page.reload();

  await t.step('The welcome is on screen');
  await t.expect.text('Welcome to Puppet Master');
  await t.expect.text('How do you work?');
  await t.expect.visible('Skip setup');

  await t.step('Skip setup');
  await t.click('Skip setup');

  await t.step('The welcome is gone and the cockpit is free');
  await t.expect.noText('How do you work?');
  await t.expect.text('No agents running.');
}
