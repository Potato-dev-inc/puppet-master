// Potato Bot's computer, end to end on stand-ins (no Linux, no model): with both Labs switches on, the bot has no computer
// until one is set up; the wizard sets up the stand-in sandbox and shows its progress to "All done"; then the bot chat has the
// Computer tab in the bot panel (closed until the monitor button in the thread header opens it) and the brain's tool call
// comes back into the chat; the sixth tool raises the "Needs you" card, which the thread announces with a banner while the
// panel is closed; and with the Sandbox switch off again the computer is gone and the bot is told it has no computer.
// The brain is the fake brain ("computer: <tool>" makes it send one sandbox_call), the sandbox is the FakeSandbox of the
// dev-fake-tui build. Nothing opens a window of its own.
import path from 'node:path';
import { makeCompanion, guarded } from '../companion/_area.mjs';
import { openSettings, goToPage } from '../settings/_area.mjs';
import { setSettings } from '../night/_area.mjs';

export const meta = {
  id: 'potato.sandbox-computer',
  title: 'With both Labs switches on, the wizard sets up the bot\'s computer and the bot uses it from the chat',
  area: 'potato',
  tags: ['smoke'],
  fresh: true,
  seed: { potato_bot_enabled: true, potato_sandbox_enabled: true },
  timeout: 240000,
};

/** Send a message to the bot the way a person does. */
async function say(t, text) {
  await t.page.fill('Message Fish', text);
  await t.press('Enter');
}

export const run = guarded(async (t) => {
  await t.step('Turn on both Labs switches (Potato Bot and Sandbox)');
  await setSettings(t, { potato_bot_enabled: true, potato_sandbox_enabled: true });

  await t.step('Make a companion called Fish and turn it into a bot');
  await makeCompanion(t, { name: 'Fish', avatar: 'bit' });
  await t.page.reload();
  await t.click('exact=Potato Bot', { within: 'css=.ck-side' });
  await t.click('exact=Make Fish a bot');
  await t.waitFor('Message Fish');

  await t.step('Nothing is set up yet: the bot is not told it has a computer');
  await say(t, 'computer: computer_observe');
  await t.expect.text('Fish has no computer.');

  await t.step('The Computer tab offers to set the computer up, and its button opens the wizard');
  await t.click('css=.ck-potato-panel-toggle');
  await t.expect.visible('css=.ck-potato-setup');
  await t.expect.text('Set up the computer', { within: 'css=.ck-potato-setup' });
  t.expect.equal(await t.page.count('css=.ck-potato-maint'), 0, 'no upkeep section before there is a computer');
  await t.click('exact=Set up the computer', { within: 'css=.ck-potato-setup' });
  await t.expect.visible('css=.ck-potato-wizard-host .sbx-sheet');
  await t.expect.text('Set up the sandbox', { within: 'css=.ck-potato-wizard-host' });
  const ctaShot = process.env.E2E_SCREENSHOT_CTA || path.join(t.outDir, 'potato-computer-setup.png');
  await t.page.screenshot(ctaShot);
  t.log('screenshot: ' + ctaShot);
  await t.press('Escape');
  await t.expect.eventually(async () => (await t.page.count('css=.ck-potato-wizard-host')) === 0, { message: 'the wizard closes' });
  await t.click('css=.ck-potato-panel-toggle');

  await t.step('Settings > Potato Bot > Sandbox > Set up');
  await openSettings(t);
  await goToPage(t, 'Potato Bot');
  await t.expect.text('Set up…');
  await t.click('exact=Set up…');
  await t.expect.text('Set up the sandbox');

  await t.step('Walk the wizard with its first answers, then Install');
  for (let i = 0; i < 8; i++) {
    if (await t.page.has('exact=Install', { within: 'css=.sbx-sheet' })) break;
    await t.click('exact=Next', { within: 'css=.sbx-sheet' });
  }
  const wizard = process.env.E2E_SCREENSHOT_WIZARD || path.join(t.outDir, 'potato-sandbox-wizard.png');
  await t.page.screenshot(wizard);
  await t.click('exact=Install', { within: 'css=.sbx-sheet' });

  await t.step('The progress reaches "All done"');
  await t.expect.text('All done', { timeout: 60000 });
  await t.expect.text('The sandbox is ready.');
  await t.click('exact=Done', { within: 'css=.sbx-sheet' });

  await t.step('Back in the bot chat the bot panel is closed; its monitor button opens the Computer tab');
  await t.click('exact=Potato Bot', { within: 'css=.ck-side' });
  await t.waitFor('Message Fish');
  t.expect.equal(await t.page.count('css=.ck-potato-panel'), 0, 'the bot panel starts closed');
  await t.click('css=.ck-potato-panel-toggle');
  await t.expect.visible('css=.ck-potato-panel');
  await t.expect.visible('css=.ck-potato-computer');
  await t.expect.text('Not in use', { within: 'css=.ck-potato-computer' });

  await t.step('The brain issues a sandbox_call and the result comes back into the chat');
  await say(t, 'computer: computer_observe');
  // the tool result is one folded line ("Used the computer · 1 step"); opened, it is a row in words, never raw text
  await t.expect.text('Used the computer · 1 step', { timeout: 30000 });
  t.expect.equal(await t.page.has('Fish used computer_observe', { within: 'css=.ck-potato-rows' }), false, 'the tool line is not a bubble');
  await t.click('css=.ck-potato-activity-head');
  await t.expect.text('Looked at the screen', { within: 'css=.ck-potato-activity' });
  await t.expect.text('Fake desktop', { within: 'css=.ck-potato-activity' });
  const activityShot = process.env.E2E_SCREENSHOT_ACTIVITY || path.join(t.outDir, 'potato-thread-activity.png');
  await t.page.screenshot(activityShot);
  t.log('screenshot: ' + activityShot);
  // (the bot lets go of the screen when its turn ends)
  await t.expect.text('Not in use', { within: 'css=.ck-potato-computer' });

  await t.step('The sixth tool, with the panel closed: a stuck bot asks the person, and the thread says "Needs you" in a banner');
  await t.click('css=.ck-potato-panel-toggle');
  await t.expect.eventually(async () => (await t.page.count('css=.ck-potato-panel')) === 0, { message: 'the bot panel is closed' });
  await say(t, 'computer: ask_person_for_screen {"reason":"The site wants a code"}');
  await t.expect.eventually(async () => (await t.page.count('css=.ck-potato-activity')) >= 2, { message: 'the second tool call is a second activity line', timeout: 30000 });
  // the banner pushes the list down when it shows, so wait for it before clicking; the first line was opened above, so the closed one is the new line
  await t.expect.visible('css=.ck-potato-banner');
  await t.click('css=.ck-potato-activity-head[aria-expanded="false"]');
  await t.expect.text('Asked you for the screen', { timeout: 30000 });
  await t.expect.visible('css=.ck-potato-banner');
  await t.expect.text('Fish needs you on the computer.', { within: 'css=.ck-potato-banner' });
  await t.expect.visible('exact=Open computer', { within: 'css=.ck-potato-banner' });
  const bannerShot = process.env.E2E_SCREENSHOT_BANNER || path.join(t.outDir, 'potato-sandbox-banner.png');
  await t.page.screenshot(bannerShot);
  t.log('screenshot: ' + bannerShot);

  await t.step('Open computer opens the panel on the Computer tab, which says "Needs you"');
  await t.click('exact=Open computer', { within: 'css=.ck-potato-banner' });
  await t.expect.eventually(async () => (await t.page.count('css=.ck-potato-banner')) === 0, { message: 'the banner is gone once the Computer tab shows' });
  // the card outlives the turn that raised it: the brain has answered and the person can still Take control or Skip
  await t.expect.text('Needs you', { within: 'css=.ck-potato-computer' });
  await t.expect.text('The site wants a code', { within: 'css=.ck-potato-computer' });
  await t.expect.visible('exact=Take control', { within: 'css=.ck-potato-computer' });
  const shot = process.env.E2E_SCREENSHOT || path.join(t.outDir, 'potato-sandbox-computer.png');
  await t.page.screenshot(shot);
  t.log('screenshot: ' + shot);

  await t.step('Skip turns the card down');
  await t.click('exact=Skip', { within: 'css=.ck-potato-computer' });
  await t.expect.eventually(async () => !(await t.page.has('exact=Skip', { within: 'css=.ck-potato-computer' })), { message: 'the card is gone after Skip' });

  await t.step('The tab has the upkeep section: Running, Stop, Restart and the way to the sandbox settings');
  await t.expect.text('Running', { within: 'css=.ck-potato-maint' });
  await t.expect.visible('exact=Stop', { within: 'css=.ck-potato-maint' });
  await t.expect.visible('exact=Restart', { within: 'css=.ck-potato-maint' });
  await t.expect.visible('exact=Open sandbox settings', { within: 'css=.ck-potato-maint' });
  const maintShot = process.env.E2E_SCREENSHOT_MAINT || path.join(t.outDir, 'potato-computer-tab.png');
  await t.page.screenshot(maintShot);
  t.log('screenshot: ' + maintShot);

  await t.step('Take control opens the computer in the full window; I am done hands it back');
  await t.click('exact=Take control', { within: 'css=.ck-potato-computer' });
  await t.expect.visible('css=.ck-potato-overlay');
  await t.expect.text('You have control', { within: 'css=.ck-potato-overlay' });
  await t.expect.text('Fish’s computer', { within: 'css=.ck-potato-overlay' });
  // (the stand-in sandbox has no screen: the full window says so in plain words and offers Retry)
  await t.expect.visible('css=.ck-potato-overlay-body');
  await t.expect.text('The sandbox is not running yet.', { within: 'css=.ck-potato-overlay' });
  await t.expect.visible('exact=Retry', { within: 'css=.ck-potato-overlay' });
  const overlayShot = process.env.E2E_SCREENSHOT_OVERLAY || path.join(t.outDir, 'potato-computer-overlay.png');
  await t.page.screenshot(overlayShot);
  t.log('screenshot: ' + overlayShot);
  await t.click("exact=I'm done", { within: 'css=.ck-potato-overlay' });
  await t.expect.eventually(async () => (await t.page.count('css=.ck-potato-overlay')) === 0, { message: 'the full window closes after I am done' });
  await t.expect.text('Not in use', { within: 'css=.ck-potato-computer' });

  await t.step('The Routines tab lists the routines (none yet) and offers a new one; a screenshot of it');
  await t.click('exact=Routines', { within: 'css=.ck-potato-panel' });
  await t.expect.text('No routines yet.', { within: 'css=.ck-potato-panel' });
  await t.expect.visible('exact=New routine', { within: 'css=.ck-potato-panel' });
  await t.expect.noText('Coming soon');
  const routinesShot = process.env.E2E_SCREENSHOT_ROUTINES || path.join(t.outDir, 'potato-panel-routines.png');
  await t.page.screenshot(routinesShot);
  t.log('screenshot: ' + routinesShot);

  await t.step('The Advanced tab has the bot settings in order; a screenshot of it');
  await t.click('exact=Advanced', { within: 'css=.ck-potato-panel' });
  const sections = ['Read replies aloud', 'Name', 'Model', 'Instructions', 'Tools', 'Notifications', 'Open work', 'Memory', 'Bot'];
  for (const title of sections) await t.expect.text(title, { within: 'css=.ck-potato-panel' });
  const titles = await t.page.c.evalJs("[...document.querySelectorAll('.ck-potato-panel .ck-potato-adv-title, .ck-potato-panel .ck-potato-psec-title')].map((h) => h.textContent)");
  t.expect.equal(JSON.stringify(titles), JSON.stringify(sections), 'the Advanced sections are in the agreed order');
  await t.expect.noText('Coming soon');
  const advancedShot = process.env.E2E_SCREENSHOT_ADVANCED || path.join(t.outDir, 'potato-panel-advanced.png');
  await t.page.screenshot(advancedShot);
  t.log('screenshot: ' + advancedShot);

  await t.step('The bot list can be made small');
  await t.click('css=.ck-potato-list-toggle');
  await t.expect.visible('css=.ck-potato-list.is-mini');
  const tabsShot = process.env.E2E_SCREENSHOT_TABS || path.join(t.outDir, 'potato-panel-routines-strip.png');
  await t.page.screenshot(tabsShot);
  t.log('screenshot: ' + tabsShot);

  await t.step('Sandbox switch off: no panel and no tools');
  await setSettings(t, { potato_sandbox_enabled: false });
  await t.expect.eventually(async () => (await t.page.count('css=.ck-potato-computer')) === 0, { message: 'the Computer panel is hidden' });
  await say(t, 'computer: browser_snapshot');
  // (the first answer is still in the chat: wait for a second one)
  await t.expect.eventually(async () => (await t.page.eval("[...document.querySelectorAll('.ck-potato-bubble--bot')].filter((e) => e.innerText.trim() === 'Fish has no computer.').length")) === 2, {
    timeout: 30000,
    message: 'the bot says again that it has no computer',
  });
});
