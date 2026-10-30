// A fresh profile shows the welcome once; Skip setup closes it. (Every other test starts with the welcome switched off.)
export const meta = {
  id: 'app.first-run-welcome',
  title: 'A fresh profile shows the welcome and Skip setup closes it',
  area: 'app',
  tags: ['first-run'],
  seed: { onboarding_done: null }, // start the app with no "welcome finished" setting
};

export async function run(t) {
  // The launcher may have opened a helper pane to get a window; the welcome stays away once this window has seen a
  // pane (localStorage). Forget that and load the page again, as a first start would.
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
