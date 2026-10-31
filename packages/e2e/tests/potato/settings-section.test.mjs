// Settings > Potato Bot: the section is there only while the Labs switch is on; it shows the brain choice, the key (never its
// value), Check key and the daily limit. The e2e app has no model: with the stand-in brain Check key says Works.
import path from 'node:path';
import { openSettings, goToPage } from '../settings/_area.mjs';
import { keepWelcomeOff } from '../companion/_area.mjs';
import { setSettings } from '../night/_area.mjs';

/** Choose an option of the select with this aria-label, the way a person's click on it ends: value set, change fired. */
async function choose(t, label, value) {
  const selector = `select[aria-label="${label}"]`;
  const ok = await t.page.eval(`(() => { const s = document.querySelector(${JSON.stringify(selector)}); if (!s) return false; Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, ${JSON.stringify(value)}); s.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
  if (!ok) throw new Error('no select called ' + label);
}

export const meta = {
  id: 'potato.settings-section',
  title: 'Settings has a Potato Bot page with the brain, the key, Check key and the daily limit',
  area: 'potato',
  tags: ['settings'],
  fresh: true,
  seed: { potato_bot_enabled: true },
  timeout: 90000,
};

export async function run(t) {
  // The harness's seed reaches the app's own copy of the settings only; the person's switch is a patch through the app.
  await setSettings(t, { potato_bot_enabled: true });

  await t.step('Open Settings: the Potato Bot page is listed');
  await openSettings(t);
  await t.expect.visible('exact=Potato Bot', { within: 'css=.st-nav' });

  await t.step('Open it');
  await goToPage(t, 'Potato Bot');
  await t.expect.text('Talk brain');
  await t.expect.text('Pays DeepSeek per message. Usually cents a day.');
  await t.expect.text('DeepSeek key');
  await t.expect.text('Default persona');
  await t.expect.text('Daily limit');

  await t.step('Check key says Works.');
  await t.click('exact=Check key');
  await t.expect.text('Works.');

  await t.step('Pick another brain: the key row says it is not needed');
  await choose(t, 'Talk brain', 'agents');
  await t.expect.text('No extra cost.');
  await t.expect.text('Not needed: this brain uses your own agents.');
  await choose(t, 'Talk brain', 'deepseek');
  await t.expect.text('Pays DeepSeek per message. Usually cents a day.');

  await t.step('Save a picture of the page');
  const shot = process.env.E2E_SCREENSHOT || path.join(t.outDir, 'potato-settings.png');
  await t.page.screenshot(shot);
  t.log('screenshot: ' + shot);
  await keepWelcomeOff(t);
}
