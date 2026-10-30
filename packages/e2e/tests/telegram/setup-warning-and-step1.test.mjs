// Settings > Phone > Telegram (early): the third-party warning always shows, and step 1 asks for a bot token in a hidden field.
// Nothing is sent: the test never presses Check and never types a real token. (needs-external for the real Telegram steps.)
import { openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'telegram.setup-warning-and-step1',
  title: 'The Telegram block warns about the third party and starts with the bot token',
  area: 'telegram',
  tags: ['telegram', 'smoke'],
};

const TOKEN = 'input[aria-label="Bot token"]';

export async function run(t) {
  await t.step('Open Settings > Phone and find Telegram');
  await openSettings(t, 'Phone');
  await t.waitFor(`css=${TOKEN}`);
  await scrollToCss(t, TOKEN);

  await t.step('The warning is on screen');
  await t.expect.text('Telegram is a third party.');
  await t.expect.text('not end-to-end encrypted');
  await t.expect.text('Don\'t use this for work you must keep private.');

  await t.step('Step 1 says how to make a bot');
  await t.expect.text('Step 1 · Make a bot');
  await t.expect.text('In Telegram, open @BotFather, send /newbot, pick a name, copy the token.');

  await t.step('The token field hides what is typed and has an off Check button');
  t.expect.equal(await t.page.eval(`document.querySelector('${TOKEN}').type`), 'password');
  t.expect.equal(await t.page.eval(`document.querySelector('${TOKEN}').placeholder`), 'Paste the token here');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Check').disabled`), true);

  await t.step('The footer says when the bot is silent and where the actions are logged');
  await t.expect.text('When your PC sleeps or Puppet Master is closed, the bot is silent.');
  await t.expect.text('See Telegram actions in Security > Audit.');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
