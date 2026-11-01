// Not a behaviour test: the Potato Bot page, dark and light, saved as pictures (see screens.test.mjs).
import { calm, shot, themeNow, restoreDark } from './_area.mjs';
import { makeCompanion } from '../companion/_area.mjs';

export const meta = {
  id: 'theme.potato',
  title: 'The Potato Bot page, dark and light, saved as pictures',
  area: 'theme',
  tags: ['theme', 'screenshots'],
  fresh: true,
  seed: { potato_bot_enabled: true },
  timeout: 150000,
};

export async function run(t) {
  await calm(t);
  try {
    await t.step('A companion that is a bot, with one exchange');
    await makeCompanion(t, { name: 'Fish', avatar: 'bit' });
    await t.page.reload();
    await t.click('exact=Potato Bot', { within: 'css=.ck-side' });
    await t.click('exact=Make Fish a bot');
    await t.waitFor('Message Fish');
    await t.page.fill('Message Fish', 'hello');
    await t.press('Enter');
    await t.expect.text('Fish heard: hello');
    await shot(t, 'dark-5-potato');

    await t.step('Light');
    await t.click('aria=Toggle theme');
    await t.sleep(500);
    t.expect.equal(await themeNow(t), 'light', 'theme is light');
    await shot(t, 'light-5-potato');
  } finally {
    await restoreDark(t);
  }
}
