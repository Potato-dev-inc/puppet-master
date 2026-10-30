// Check stays off for an empty or blank field, turns on with text, and off again when the text is removed. Never pressed.
import { openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'telegram.check-needs-a-token',
  title: 'Check is off for an empty or blank token field and on once something is typed',
  area: 'telegram',
  tags: ['telegram'],
  edge: true,
};

const TOKEN = 'input[aria-label="Bot token"]';
const checkOff = (t) => t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Check').disabled`);
const typed = (t) => t.page.eval(`document.querySelector('${TOKEN}').value`);

export async function run(t) {
  await openSettings(t, 'Phone');
  await t.waitFor(`css=${TOKEN}`);
  await scrollToCss(t, TOKEN);
  t.expect.equal(await checkOff(t), true, 'off when empty');

  await t.step('Spaces only: still off');
  await t.fill(`css=${TOKEN}`, '    ');
  await t.expect.eventually(async () => (await typed(t)) === '    ', { message: 'the spaces are in the field' });
  t.expect.equal(await checkOff(t), true, 'off for spaces');

  await t.step('A made-up value that is not a token: Check turns on (it is not pressed)');
  await t.fill(`css=${TOKEN}`, 'not-a-real-token');
  await t.expect.eventually(async () => (await checkOff(t)) === false, { message: 'Check is enabled' });

  await t.step('Remove the text: off again, and nothing was saved');
  await t.fill(`css=${TOKEN}`, '');
  await t.expect.eventually(async () => (await checkOff(t)) === true, { message: 'Check is disabled again' });
  await t.expect.noText('Token saved');
  await t.expect.noText('Step 2 · Connect your chat');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
