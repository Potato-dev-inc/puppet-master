// Helpers for the theme screenshot tests. Files starting with "_" are never run as tests.
export * from '../app/_area.mjs';
import path from 'node:path';
import fs from 'node:fs';

/**
 * Take a picture of the window. Files go to THEME_SHOTS_DIR (default: this test's folder) and start with THEME_SHOT_PREFIX
 * (for example "before" or "after"), so two runs can be put side by side.
 */
export async function shot(t, name) {
  const dir = process.env.THEME_SHOTS_DIR || t.outDir;
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${process.env.THEME_SHOT_PREFIX || 'shot'}-${name}.png`);
  await t.page.pause(350); // let the last transition finish
  await t.page.screenshot(file);
  t.log('screenshot: ' + file);
  return file;
}
