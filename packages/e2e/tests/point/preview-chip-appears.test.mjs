// Point and Fix: when an agent prints the address of a local dev server, the pane gets a Preview chip.
import { startDevServer, serverPane, selectByTitle, projectDir } from './_area.mjs';

export const meta = {
  id: 'point.preview-chip-appears',
  title: 'A local address printed by an agent gives the pane a Preview chip and palette actions',
  area: 'point',
  tags: ['point', 'smoke'],
};

export async function run(t) {
  const site = await startDevServer();
  try {
    await t.step('One agent has not started a server: no Preview chip');
    t.fake.script('boot_ms = 100\ninline = true\n');
    await t.spawnPane('nimbus', { cwd: projectDir(t, 'quiet') });
    await t.click('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', { nth: 0 });
    await t.expect.visible('exact=Terminal');
    await t.expect.notVisible('exact=Preview');

    await t.step('Another agent prints "Local: http://localhost:<port>/"');
    await serverPane(t, site.port);
    await selectByTitle(t, 'serve the app');

    await t.step('Its header gets a Preview chip');
    await t.expect.visible('exact=Preview');

    await t.step('The command palette offers to open the preview and to point and fix in it');
    await t.press('Ctrl+K');
    await t.type('preview');
    await t.expect.text(`Open preview:`);
    await t.expect.text(`Point and fix in the preview:`);
    await t.expect.text(`:${site.port}`);
    await t.press('Escape');
  } finally {
    await site.close();
  }
}
