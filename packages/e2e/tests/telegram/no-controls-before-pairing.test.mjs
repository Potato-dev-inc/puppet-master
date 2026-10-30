// Until a token is saved and a chat is connected, the Telegram block offers no switches and no Disconnect.
import { openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'telegram.no-controls-before-pairing',
  title: 'Before a bot is connected there are no Telegram switches and nothing to disconnect',
  area: 'telegram',
  tags: ['telegram'],
  edge: true,
};

export async function run(t) {
  await openSettings(t, 'Phone');
  await scrollToCss(t, 'input[aria-label="Bot token"]');

  await t.step('Only step 1 is there');
  await t.expect.text('Step 1 · Make a bot');
  await t.expect.noText('Step 2 · Connect your chat');

  await t.step('None of the later controls are drawn');
  for (const text of ['Telegram on', 'What Telegram may do', 'See status and replies', 'Answer questions', 'Send messages to agents', 'Tell me when a run finishes', 'Short messages only']) {
    await t.expect.noText(text);
  }
  await t.expect.notVisible('exact=Disconnect');
  await t.expect.notVisible('exact=Make a code');

  await t.step('The warning that never goes away is still the first thing in the block');
  await t.expect.text('Everything this bot sends and receives is stored on Telegram\'s servers');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';
