// Potato Bot's computer, end to end on stand-ins (no Linux, no model): with both Labs switches on, the bot has no computer
// until one is set up; the wizard sets up the stand-in sandbox and shows its progress to "All done"; then the bot chat has the
// Computer panel and the brain's tool call comes back into the chat; the sixth tool raises the "Needs you" card; and with the
// Sandbox switch off again the panel is gone and the bot is told it has no computer.
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

  await t.step('Back in the bot chat there is a Computer panel');
  await t.click('exact=Potato Bot', { within: 'css=.ck-side' });
  await t.waitFor('Message Fish');
  await t.expect.visible('css=.ck-potato-computer');
  await t.expect.text('Not in use', { within: 'css=.ck-potato-computer' });

  await t.step('The brain issues a sandbox_call and the result comes back into the chat');
  await say(t, 'computer: computer_observe');
  await t.expect.text('Fish used computer_observe: ok, saw Fake desktop', { timeout: 30000 });
  // (the bot lets go of the screen when its turn ends)
  await t.expect.text('Not in use', { within: 'css=.ck-potato-computer' });

  await t.step('The sixth tool: a stuck bot asks the person, and the panel says "Needs you"');
  await say(t, 'computer: ask_person_for_screen {"reason":"The site wants a code"}');
  await t.expect.text('Fish used ask_person_for_screen: ok', { timeout: 30000 });
  // the card outlives the turn that raised it: the brain has answered and the person can still Take over or Skip
  await t.expect.text('Needs you', { within: 'css=.ck-potato-computer' });
  await t.expect.text('The site wants a code', { within: 'css=.ck-potato-computer' });
  await t.expect.visible('exact=Take over', { within: 'css=.ck-potato-computer' });
  const shot = process.env.E2E_SCREENSHOT || path.join(t.outDir, 'potato-sandbox-computer.png');
  await t.page.screenshot(shot);
  t.log('screenshot: ' + shot);

  await t.step('Skip turns the card down');
  await t.click('exact=Skip', { within: 'css=.ck-potato-computer' });
  await t.expect.eventually(async () => !(await t.page.has('exact=Skip', { within: 'css=.ck-potato-computer' })), { message: 'the card is gone after Skip' });

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
