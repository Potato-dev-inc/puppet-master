// Potato Bot, the chat page: a message reaches the bot, "Fish is typing…" shows, the reply and the mood come back,
// and the voice switch adds its note row. The brain is the fake brain (no model, no key, no network): the launcher
// points PM_POTATO_BRAIN at packages/potato-brain/test/fake-brain.mjs, which a dev-fake-tui build reads.
import path from 'node:path';
import { makeCompanion, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'potato.dm-reply',
  title: 'A message to a Potato Bot gets a typing row, a reply and a mood, and the voice switch says so',
  area: 'potato',
  tags: ['smoke'],
  fresh: true,
  seed: { potato_bot_enabled: true },
  timeout: 120000,
};

export const run = guarded(async (t) => {
  await t.step('Make a companion called Fish: a bot is a companion with a brain');
  await makeCompanion(t, { name: 'Fish', avatar: 'bit' });
  await t.page.reload();

  await t.step('Open Potato Bot from the sidebar');
  await t.click('exact=Potato Bot', { within: 'css=.ck-side' });
  await t.expect.text('No bots yet. Make one to start chatting.');

  await t.step('Turn the companion into a bot');
  await t.click('exact=Make Fish a bot');
  await t.waitFor('Message Fish');
  await t.expect.visible('exact=Fish', { within: 'css=.ck-potato-list' });
  await t.expect.visible('exact=Potato');

  await t.step('Send "hello"');
  await t.page.fill('Message Fish', 'hello');
  await t.press('Enter');

  await t.step('The typing row shows while the brain thinks');
  await t.expect.text('Fish is typing…');

  await t.step('The reply and the mood come back');
  await t.expect.text('Fish heard: hello');
  await t.expect.text('Genki · energy 60');
  await t.expect.noText('Fish is typing…');
  t.expect.equal(await t.page.count('css=.ck-potato-bubble--you'), 1, 'one bubble is yours');
  t.expect.equal(await t.page.count('css=.ck-potato-bubble--bot'), 1, 'one bubble is the bot\'s');

  await t.step('Switch the voice to Potato');
  await t.click('exact=Potato');
  await t.expect.text('Now talking as Potato.');
  await t.expect.text('Fish heard: hello');

  await t.step('Save a picture of the page');
  const shot = process.env.E2E_SCREENSHOT || path.join(t.outDir, 'potato-page.png');
  await t.page.screenshot(shot);
  t.log('screenshot: ' + shot);
});
