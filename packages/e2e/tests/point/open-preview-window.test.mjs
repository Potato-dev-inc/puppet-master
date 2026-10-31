// Point and Fix: the Preview chip opens the page in its own window inside the app.
import { startDevServer, serverPane, openPreviewWindow, closeExtraWindows, selectByTitle } from './_area.mjs';

export const meta = {
  id: 'point.open-preview-window',
  title: 'The Preview chip opens the dev server page in its own window',
  area: 'point',
  tags: ['point'],
};

export async function run(t) {
  const site = await startDevServer();
  try {
    await serverPane(t, site.port);
    await selectByTitle(t, 'serve the app');

    await t.step('Press the Preview chip');
    const preview = await openPreviewWindow(t, site.port);

    await t.step('A window with the page opens: its title and its text are the dev server\'s');
    await t.expect.eventually(async () => (await preview.eval('document.title')) === 'Demo shop', { message: 'the window shows the page title' });
    t.expect.includes(await preview.eval('document.body.innerText'), 'Fresh bread every morning.');

    await t.step('The picker is not on until you ask for it');
    t.expect.equal(await preview.eval(`!!document.getElementById('pm-point-cursor')`), false);
    preview.close();
  } finally {
    await closeExtraWindows(t);
    await site.close();
  }
}
