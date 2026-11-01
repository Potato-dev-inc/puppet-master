// Not a behaviour test: it walks the main desktop screens and saves a picture of each, dark and light, so a change of
// the colour tokens can be looked at before and after (THEME_SHOT_PREFIX=before|after, THEME_SHOTS_DIR=<folder>).
import { calm, openPane, askScript, slowScript, shot, themeNow, restoreDark } from './_area.mjs';
import { openSettings } from '../settings/_area.mjs';

export const meta = {
  id: 'theme.screens',
  title: 'The main screens, dark and light, saved as pictures',
  area: 'theme',
  tags: ['theme', 'screenshots'],
  timeout: 180000,
};

const IDLE_SCRIPT = `
boot_ms = 100
inline = true
`;

async function toggleTheme(t) {
  await t.click('aria=Toggle theme');
  await t.sleep(500);
}

export async function run(t) {
  await calm(t);
  try {
    await t.step('Three agents: idle, working, waiting for an answer');
    const ask = await t.spawnPane('nimbus', { script: askScript });
    await ask.write('make it');
    await t.fake.waitEvent('approval_shown');
    await t.expect.eventually(async () => (await ask.info())?.status === 'waiting_input', { message: 'the app says waiting', timeout: 20000 });
    const busy = await t.spawnPane('nimbus', { script: slowScript(90000, 'done') });
    await busy.write('refactor the parser');
    await t.expect.eventually(async () => (await busy.info())?.status === 'running', { message: 'the app says working', timeout: 20000 });
    await t.spawnPane('nimbus', { script: IDLE_SCRIPT });

    for (const mode of ['dark', 'light']) {
      if (mode === 'light') await toggleTheme(t);
      t.expect.equal(await themeNow(t), mode, 'theme is ' + mode);

      await t.step(`Inbox (${mode})`);
      await t.click('exact=Inbox', { within: 'css=.ck-side' });
      await shot(t, `${mode}-1-inbox`);

      await t.step(`A pane that needs you (${mode})`);
      await t.click('css=.ck-side .ck-row.is-needs');
      await t.waitFor('css=.ck-pane-head');
      await shot(t, `${mode}-2-needs-you`);

      await t.step(`Chat tab with the composer (${mode})`);
      await t.click('exact=Chat');
      await t.waitFor('aria=Message to the agent');
      await t.page.fill('aria=Message to the agent', 'Please also add tests for the empty case');
      await shot(t, `${mode}-3-chat`);
      await t.page.fill('aria=Message to the agent', '');

      await t.step(`Settings (${mode})`);
      await openSettings(t);
      await shot(t, `${mode}-4-settings`);
      await t.click('exact=Inbox', { within: 'css=.ck-side' });
    }
  } finally {
    await restoreDark(t);
  }
}
