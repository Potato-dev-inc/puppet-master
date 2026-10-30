// The token field is a password field with autocomplete off, so what is typed is never shown or offered back; text typed
// into it is just a draft (never saved here: the test does not press Check).
import { openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'telegram.token-not-kept-on-screen',
  title: 'The bot token field is hidden, has autocomplete off and keeps no text after leaving',
  area: 'telegram',
  tags: ['telegram'],
  edge: true,
};

const TOKEN = 'input[aria-label="Bot token"]';
const state = (t) => t.page.eval(`(() => { const i = document.querySelector('${TOKEN}'); return { type: i.type, autocomplete: i.autocomplete, spellcheck: i.spellcheck, value: i.value }; })()`);

export async function run(t) {
  await openSettings(t, 'Phone');
  await t.waitFor(`css=${TOKEN}`);
  await scrollToCss(t, TOKEN);

  await t.step('Type a made-up value');
  await t.fill(`css=${TOKEN}`, 'draft-value-1234');
  let s = await state(t);
  t.expect.equal(s.type, 'password');
  t.expect.equal(s.autocomplete, 'off');
  t.expect.equal(s.spellcheck, false);
  t.expect.equal(s.value, 'draft-value-1234');
  await t.expect.noText('draft-value-1234');

  await t.step('Leave Settings and come back: the draft is gone');
  await t.click('exact=Inbox');
  await openSettings(t, 'Phone');
  await t.waitFor(`css=${TOKEN}`);
  s = await state(t);
  t.expect.equal(s.value, '');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
